.PHONY: pre-commit install-hooks

pre-commit:
	./.githooks/pre-commit

install-hooks:
	git config core.hooksPath .githooks
