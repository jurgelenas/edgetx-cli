BINARY  := edgetx-cli
BIN_DIR := bin
MODULE  := github.com/jurgelenas/edgetx-cli

GO      := go
GOFLAGS :=

# WAMR setup paths
WAMR_GO_PKG := $(shell $(GO) list -m -f '{{.Dir}}' github.com/bytecodealliance/wasm-micro-runtime/language-bindings/go 2>/dev/null)
WAMR_SRC    := $(shell find $$($(GO) env GOMODCACHE)/github.com/bytecodealliance/wasm-micro-runtime@* -maxdepth 0 -type d 2>/dev/null | head -1)

.PHONY: all build test test-verbose lint clean tidy setup-wamr

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

# Build WAMR from source and install headers/library into the Go package's
# packaged directory. Required before building the simulator.
# Prerequisites: cmake, g++, libsdl2-dev
setup-wamr:
	@echo "Copying WAMR source to writable location..."
	@chmod -R u+w /tmp/wamr-src 2>/dev/null || true
	@rm -rf /tmp/wamr-src /tmp/wamr-build
	cp -r "$(WAMR_SRC)" /tmp/wamr-src
	chmod -R u+w /tmp/wamr-src
	@echo "Building WAMR runtime library..."
	@mkdir -p /tmp/wamr-build
	cd /tmp/wamr-build && cmake /tmp/wamr-src/product-mini/platforms/linux \
		-DWAMR_BUILD_INTERP=1 \
		-DWAMR_BUILD_FAST_INTERP=0 \
		-DWAMR_BUILD_AOT=0 \
		-DWAMR_BUILD_JIT=0 \
		-DWAMR_BUILD_LIBC_BUILTIN=1 \
		-DWAMR_BUILD_LIBC_WASI=1 \
		-DWAMR_BUILD_LIB_WASI_THREADS=1 \
		-DWAMR_BUILD_LIB_PTHREAD=1 \
		-DWAMR_BUILD_SHARED_MEMORY=1 \
		-DWAMR_BUILD_BULK_MEMORY=1 \
		-DWAMR_BUILD_REF_TYPES=1 \
		-DWAMR_BUILD_SIMD=0 \
		-DWAMR_BUILD_EXCE_HANDLING=1 \
		-DWAMR_DISABLE_HW_BOUND_CHECK=1 \
		-DWAMR_BUILD_DUMP_CALL_STACK=1 \
		-DWAMR_BUILD_MEMORY_PROFILING=1 \
		-DCMAKE_BUILD_TYPE=Release
	$(MAKE) -C /tmp/wamr-build -j$$(nproc)
	@echo "Installing WAMR headers and library..."
	@chmod -R u+w "$(WAMR_GO_PKG)/wamr/packaged/include/" 2>/dev/null || true
	@chmod -R u+w "$(WAMR_GO_PKG)/wamr/packaged/lib/linux-amd64/" 2>/dev/null || true
	@mkdir -p "$(WAMR_GO_PKG)/wamr/packaged/lib/linux-amd64/"
	cp /tmp/wamr-src/core/iwasm/include/wasm_export.h "$(WAMR_GO_PKG)/wamr/packaged/include/"
	cp /tmp/wamr-src/core/iwasm/include/lib_export.h "$(WAMR_GO_PKG)/wamr/packaged/include/"
	cp /tmp/wamr-build/libiwasm.a "$(WAMR_GO_PKG)/wamr/packaged/lib/linux-amd64/"
	@echo "WAMR setup complete."
