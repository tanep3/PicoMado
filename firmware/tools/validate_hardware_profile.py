#!/usr/bin/env python3
"""Validate the compiler-visible hardware macros against one verified profile."""
import argparse
import os
from pathlib import Path
import re
import subprocess
import sys

DIRECTIVE = re.compile(r'^\s*#\s*(define|undef)\s+(PM_[A-Za-z0-9_]+)\b')
MACRO = re.compile(r'^#define\s+(PM_[A-Za-z0-9_]+)\s+(.*)$')
PREFIXES = ('PM_LCD_', 'PM_EINK_', 'PM_BUTTON_', 'PM_DISPLAY_', 'PM_PIXEL_')
OPTIONAL = {'PM_LCD_BLK_CONNECTED', 'PM_LCD_BLK_ACTIVE_HIGH', 'PM_EINK_PANEL_CONNECTED'}


def directive_text(body):
    """Remove comments and C line splices while preserving literal strings."""
    body = re.sub(r'\\\r?\n', '', body)
    result = []
    state = 'code'
    i = 0
    while i < len(body):
        char = body[i]
        nxt = body[i + 1] if i + 1 < len(body) else ''
        if state == 'line':
            if char == '\n':
                result.append(char)
                state = 'code'
            else:
                result.append(' ')
        elif state == 'block':
            if char == '*' and nxt == '/':
                result.extend('  ')
                i += 1
                state = 'code'
            else:
                result.append('\n' if char == '\n' else ' ')
        elif state in ('string', 'character'):
            result.append(char)
            if char == '\\' and nxt:
                result.append(nxt)
                i += 1
            elif char == ('"' if state == 'string' else "'"):
                state = 'code'
        elif char == '/' and nxt == '/':
            result.extend('  ')
            i += 1
            state = 'line'
        elif char == '/' and nxt == '*':
            result.extend('  ')
            i += 1
            state = 'block'
        else:
            result.append(char)
            if char == '"':
                state = 'string'
            elif char == "'":
                state = 'character'
        i += 1
    if state == 'block':
        raise ValueError('unterminated config comment')
    return ''.join(result)


def source_directives(body):
    clean = directive_text(body)
    return [(match.group(1), match.group(2).strip()) for line in clean.splitlines()
            if (match := re.match(r'^\s*#\s*([A-Za-z_][A-Za-z_0-9]*)\b(.*)$', line))]


def require_flat_source(body, profile_name=None):
    entries = source_directives(body)
    if any(command not in ('define', 'pragma', 'include') for command, _ in entries):
        raise ValueError('conditional or unsupported preprocessor directive in hardware source')
    includes = [argument for command, argument in entries if command == 'include']
    expected = [] if profile_name is None else ['"' + profile_name + '.h"']
    if includes != expected:
        raise ValueError('only the selected profile include is allowed')
    if any(command == 'pragma' and argument != 'once' for command, argument in entries):
        raise ValueError('unsupported hardware source pragma')
    return directive_text(body)


def protected(name):
    return name.startswith(PREFIXES) and name not in OPTIONAL


def directives(body):
    return [(match.group(1), match.group(2)) for line in body.splitlines()
            if (match := DIRECTIVE.match(line))]


def compiler_output(compiler, path, profile_dir, option):
    result = subprocess.run([compiler, '-E', option, '-x', 'c', '-I', str(profile_dir),
                             '-include', str(path), os.devnull], capture_output=True,
                            timeout=20, check=False)
    if result.returncode:
        raise ValueError('hardware preprocessing failed')
    return result.stdout.decode('utf-8', errors='replace')


def effective(body):
    result = {}
    for line in body.splitlines():
        match = MACRO.match(line)
        if match and protected(match.group(1)):
            result[match.group(1)] = match.group(2).strip()
    return result


def validate(config, profile, compiler):
    config, profile = Path(config).resolve(), Path(profile).resolve()
    name = profile.stem
    source = config.read_text()
    profile_source = profile.read_text()
    require_flat_source(profile_source)
    profile_directives = directives(directive_text(profile_source))
    profile_names = [name for action, name in profile_directives if protected(name)]
    if len(profile_names) != len(set(profile_names)):
        raise ValueError('duplicate profile hardware definition')
    profile_definitions = effective(compiler_output(compiler, profile, profile.parent, '-dM'))
    if not profile_definitions:
        raise ValueError('empty hardware profile')
    include_style = any(command == 'include' for command, _ in source_directives(source))
    require_flat_source(source, name if include_style else None)
    local = directives(directive_text(source))
    if include_style:
        if any(protected(macro) for _, macro in local):
            raise ValueError('device config changes profile-owned hardware')
    else:
        # Existing private configs can remain unchanged only if all their hardware
        # declarations occur exactly once and match the selected profile.
        names = [macro for _, macro in local if protected(macro)]
        if len(names) != len(set(names)) or set(names) != set(profile_definitions):
            raise ValueError('legacy hardware definitions are incomplete or duplicated')
        if any(action == 'undef' for action, macro in local if protected(macro)):
            raise ValueError('legacy hardware undefinition')
    allowed = {'PM_LCD_BLK_CONNECTED', 'PM_LCD_BLK_ACTIVE_HIGH'} if 'lcd' in name else {'PM_EINK_PANEL_CONNECTED'}
    optional = [(action, macro) for action, macro in local if macro in OPTIONAL]
    if any(action != 'define' or macro not in allowed for action, macro in optional):
        raise ValueError('device config changes unsupported optional hardware')
    if len(optional) != len({macro for _, macro in optional}):
        raise ValueError('duplicate optional hardware definition')
    actual = effective(compiler_output(compiler, config, profile.parent, '-dM'))
    if actual != profile_definitions:
        raise ValueError('effective hardware macros differ from selected profile')
    expanded = directives(compiler_output(compiler, config, profile.parent, '-dD'))
    active = [(action, macro) for action, macro in expanded if protected(macro)]
    if any(action != 'define' for action, _ in active) or len(active) != len(set(macro for _, macro in active)):
        raise ValueError('hardware macro redefined or undefined through an include')
    if set(macro for _, macro in active) != set(profile_definitions):
        raise ValueError('hardware macro set differs from profile')
    all_macros = compiler_output(compiler, config, profile.parent, '-dM')
    settings = {m.group(1): m.group(2).strip() for line in all_macros.splitlines()
                if (m := MACRO.match(line)) and m.group(1) in OPTIONAL}
    if any(k not in allowed or v not in ('0', '1') for k, v in settings.items()):
        raise ValueError('invalid optional hardware setting')


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--config', required=True)
    parser.add_argument('--profile', required=True)
    parser.add_argument('--compiler', required=True)
    args = parser.parse_args()
    try:
        validate(args.config, args.profile, args.compiler)
    except (OSError, ValueError, subprocess.TimeoutExpired) as problem:
        print('hardware profile rejected: ' + str(problem), file=sys.stderr)
        return 1
    print('hardware profile validated')
    return 0


if __name__ == '__main__':
    raise SystemExit(main())
