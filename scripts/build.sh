#!/bin/bash

# MOQ C++ API Development Script
# Usage: ./scripts/build.sh [clean|release|debug|example]

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
BUILD_DIR="$PROJECT_ROOT/build"

function usage() {
    echo "Usage: $0 [clean|release|debug|example|install]"
    echo ""
    echo "Commands:"
    echo "  clean     - Clean build directory"
    echo "  debug     - Build in debug mode (default)"
    echo "  release   - Build in release mode"
    echo "  example   - Build and run example"
    echo "  install   - Install to system"
    echo ""
}

function clean_build() {
    echo "🧹 Cleaning build directory..."
    rm -rf "$BUILD_DIR"
    echo "✅ Clean complete"
}

function configure_cmake() {
    local build_type=${1:-Debug}
    echo "🔧 Configuring CMake (${build_type})..."
    
    mkdir -p "$BUILD_DIR"
    cd "$BUILD_DIR"
    
    cmake .. -DCMAKE_BUILD_TYPE="$build_type"
    echo "✅ Configuration complete"
}

function build_project() {
    echo "🔨 Building project..."
    cd "$BUILD_DIR"
    
    if command -v ninja >/dev/null 2>&1; then
        ninja
    else
        make -j$(nproc 2>/dev/null || sysctl -n hw.ncpu 2>/dev/null || echo 4)
    fi
    
    echo "✅ Build complete"
}

function run_example() {
    echo "🚀 Running example..."
    cd "$BUILD_DIR"
    
    if [[ "$OSTYPE" == "msys" || "$OSTYPE" == "win32" ]]; then
        ./moq_example.exe
    else
        ./moq_example
    fi
}

function install_project() {
    echo "📦 Installing project..."
    cd "$BUILD_DIR"
    
    if [[ "$OSTYPE" == "darwin"* ]]; then
        sudo cmake --install . --prefix /usr/local
    elif [[ "$OSTYPE" == "linux-gnu"* ]]; then
        sudo cmake --install . --prefix /usr/local
    else
        cmake --install . --prefix /usr/local
    fi
    
    echo "✅ Installation complete"
}

# Main script logic
case "${1:-debug}" in
    clean)
        clean_build
        ;;
    debug)
        configure_cmake "Debug"
        build_project
        ;;
    release)
        configure_cmake "Release"
        build_project
        ;;
    example)
        if [[ ! -d "$BUILD_DIR" ]]; then
            configure_cmake "Debug"
        fi
        build_project
        run_example
        ;;
    install)
        if [[ ! -d "$BUILD_DIR" ]]; then
            configure_cmake "Release"
            build_project
        fi
        install_project
        ;;
    help|--help|-h)
        usage
        ;;
    *)
        echo "❌ Unknown command: $1"
        usage
        exit 1
        ;;
esac
