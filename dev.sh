#!/bin/bash
# Development helper script for moq-cpp

set -e

case "$1" in
    "fmt")
        echo "🎨 Formatting Rust code..."
        cargo fmt
        echo "✅ Code formatted!"
        ;;
    
    "check")
        echo "🔍 Running format check..."
        cargo fmt --check
        echo "🔍 Running clippy..."
        cargo clippy --all-targets --all-features -- -D warnings
        echo "✅ All checks passed!"
        ;;
    
    "fix")
        echo "🔧 Auto-fixing code..."
        cargo fmt
        cargo clippy --all-targets --all-features --fix --allow-dirty --allow-staged
        echo "✅ Auto-fixes applied!"
        ;;
    
    "build")
        echo "🔨 Building project..."
        cargo fmt
        cargo clippy --all-targets --all-features -- -D warnings
        cmake --build build --config Release
        echo "✅ Build complete!"
        ;;
    
    "clean")
        echo "🧹 Cleaning build artifacts..."
        cargo clean
        rm -rf build
        echo "✅ Clean complete!"
        ;;
    
    "setup-hooks")
        echo "⚙️  Setting up Git hooks..."
        git config core.hooksPath .githooks
        chmod +x .githooks/pre-commit
        echo "✅ Git hooks configured!"
        ;;
    
    *)
        echo "Usage: $0 {fmt|check|fix|build|clean|setup-hooks}"
        echo ""
        echo "Commands:"
        echo "  fmt         - Format Rust code"
        echo "  check       - Check formatting and run clippy"
        echo "  fix         - Auto-fix formatting and clippy issues"
        echo "  build       - Format, check, and build project"
        echo "  clean       - Clean build artifacts"
        echo "  setup-hooks - Configure Git pre-commit hooks"
        exit 1
        ;;
esac