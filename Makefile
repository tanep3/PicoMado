.PHONY: check static
check:
	python3 tools/project_gate.py check
static:
	python3 tools/project_gate.py static
