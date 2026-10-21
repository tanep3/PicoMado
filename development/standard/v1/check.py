"""Configurable dependency/ownership checks; no project domain assumptions."""
import ast
import json
import re
from pathlib import Path


def check(root, config):
    root=Path(root);errors=[]
    for item in config['python']:
        path=item['path'];file=root/path
        if not file.exists():errors.append(path+': missing governed module');continue
        tree=ast.parse(file.read_text(),filename=path)
        parents={child:node for node in ast.walk(tree) for child in ast.iter_child_nodes(node)}
        for node in ast.walk(tree):
            message=None
            if isinstance(node,(ast.Import,ast.ImportFrom)) and 'imports' in item:
                names=[n.name.split('.')[0]for n in node.names] if isinstance(node,ast.Import)else[(node.module or '').split('.')[0]]
                if any(n not in item['imports']for n in names):message='forbidden dependency '+','.join(names)
            if item.get('adapter'):
                if isinstance(node,ast.AugAssign)and isinstance(node.op,ast.BitOr):message='adapter cannot merge outcome state with |='
                if isinstance(node,ast.keyword)and node.arg=='complete':message='adapter cannot construct completion'
                if isinstance(node,ast.Dict)and any(isinstance(k,ast.Constant)and k.value=='complete'for k in node.keys):message='adapter cannot construct completion'
                if isinstance(node,ast.Subscript)and isinstance(node.ctx,ast.Store)and isinstance(node.slice,ast.Constant)and node.slice.value=='complete':message='adapter cannot mutate completion'
                if isinstance(node,ast.Attribute)and isinstance(node.ctx,ast.Store)and ('__state'in node.attr or node.attr in ['phase','state']):message='adapter cannot mutate aggregate state'
            if isinstance(node,ast.Call):
                name=ast.unparse(node.func)
                if name in ['eval','exec','dict.__setitem__','setattr','object.__setattr__']:
                    owner=node
                    while owner in parents and not isinstance(owner,(ast.FunctionDef,ast.AsyncFunctionDef)):owner=parents[owner]
                    initializing=isinstance(owner,ast.FunctionDef)and owner.name=='__init__'and name in item.get('initialization_calls',[])
                    if not initializing:message='dynamic ownership bypass'
                for forbidden in config.get('restricted_calls',[]):
                    if name.split('.')[-1]==forbidden['name']:
                        owner=node
                        while owner in parents and not isinstance(owner,(ast.FunctionDef,ast.AsyncFunctionDef)):owner=parents[owner]
                        fn=owner.name if isinstance(owner,(ast.FunctionDef,ast.AsyncFunctionDef))else None
                        if [path,fn]not in forbidden['allowed']:message='restricted capability/construction '+name
            if message:errors.append(f'{path}:{getattr(node,"lineno",0)}: {message}')
    for item in config.get('writes',[]):
        path=item['path'];text=(root/path).read_text()
        # Strip literals/comments while retaining offsets for function ownership.
        clean=re.sub(r'"(?:\\.|[^"\\])*"|\'(?:\\.|[^\'\\])*\'|//[^\n]*|/\*[\s\S]*?\*/',lambda m:' '*len(m[0]),text)
        ranges=[]
        for m in re.finditer(item['function_pattern'],clean):
            start=clean.index('{',m.start());depth=1;end=start+1
            while end<len(clean)and depth:
                depth+=(clean[end]=='{')-(clean[end]=='}');end+=1
            ranges.append((start,end,m.group(1)))
        for m in re.finditer(item['pattern'],clean):
            owners=[name for a,b,name in ranges if a<=m.start()<b]
            if not owners or owners[-1]not in item['allowed']:
                errors.append(f'{path}:{clean[:m.start()].count(chr(10))+1}: state write outside aggregate transition')
    for item in config.get('delegations',[]):
        match=re.search(item['pattern'],(root/item['path']).read_text())
        if not match or re.sub(r'\s+','',match.group(1))!=re.sub(r'\s+','',item['body']):
            errors.append(item['path']+': identity adapter must delegate to model projection')
    return errors


if __name__=='__main__':
    import argparse
    p=argparse.ArgumentParser();p.add_argument('--root',type=Path,required=True);p.add_argument('--config',type=Path,required=True);a=p.parse_args()
    errors=check(a.root,json.loads(a.config.read_text()))
    if errors:raise SystemExit('\n'.join(errors))
