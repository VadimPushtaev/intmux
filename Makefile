.PHONY: pre-commit install-hooks static-checks

pre-commit:
	./.githooks/pre-commit

static-checks: pre-commit

install-hooks:
	git config core.hooksPath .githooks
