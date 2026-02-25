BINARY  := edgetx
BIN_DIR := bin
MODULE  := github.com/edgetx/cli

GO      := go
GOFLAGS :=

.PHONY: all build test test-verbose lint clean tidy

all: tidy build test

build:
	$(GO) build $(GOFLAGS) -o $(BIN_DIR)/$(BINARY) .

test:
	$(GO) test ./...

test-verbose:
	$(GO) test -v ./...

lint:
	$(GO) vet ./...

clean:
	rm -f $(BIN_DIR)/$(BINARY)

tidy:
	$(GO) mod tidy
