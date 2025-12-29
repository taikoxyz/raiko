#!/usr/bin/env bash

# Script to manage zkVM versions across the project
# Usage: ./script/update-zkvm-versions.sh --zkvm <sp1|risc0|zisk> [options]

set -e

ZKVM=""
SHOW_CURRENT=false

# SP1 versions
SP1_SDK_VERSION=""
SP1_PROVER_VERSION=""
SP1_ZKVM_VERSION=""
SP1_PRIMITIVES_VERSION=""
SP1_HELPER_VERSION=""
SP1_CURVES_VERSION=""

# RISC0 versions  
RISC0_ZKVM_VERSION=""
RISC0_PLATFORM_VERSION=""
RISC0_BINFMT_VERSION=""
BONSAI_SDK_VERSION=""

# Zisk version (git ref)
ZISK_GIT_REF=""

# Parse arguments
while [[ $# -gt 0 ]]; do
  case $1 in
    --zkvm)
      ZKVM="$2"
      shift 2
      ;;
    --sp1-sdk)
      SP1_SDK_VERSION="$2"
      shift 2
      ;;
    --sp1-zkvm)
      SP1_ZKVM_VERSION="$2"
      shift 2
      ;;
    --sp1-prover)
      SP1_PROVER_VERSION="$2"
      shift 2
      ;;
    --sp1-primitives)
      SP1_PRIMITIVES_VERSION="$2"
      shift 2
      ;;
    --sp1-helper)
      SP1_HELPER_VERSION="$2"
      shift 2
      ;;
    --risc0-zkvm)
      RISC0_ZKVM_VERSION="$2"
      shift 2
      ;;
    --risc0-platform)
      RISC0_PLATFORM_VERSION="$2"
      shift 2
      ;;
    --risc0-binfmt)
      RISC0_BINFMT_VERSION="$2"
      shift 2
      ;;
    --bonsai-sdk)
      BONSAI_SDK_VERSION="$2"
      shift 2
      ;;
    --zisk-ref)
      ZISK_GIT_REF="$2"
      shift 2
      ;;
    --show-current)
      SHOW_CURRENT=true
      shift
      ;;
    --help)
      echo "Usage: $0 --zkvm <sp1|risc0|zisk> [options]"
      echo ""
      echo "Options:"
      echo "  --zkvm <sp1|risc0|zisk>  Select zkVM to update"
      echo "  --show-current            Show current versions only"
      echo ""
      echo "SP1 options:"
      echo "  --sp1-sdk <version>       Update sp1-sdk version"
      echo "  --sp1-zkvm <version>      Update sp1-zkvm version"
      echo "  --sp1-prover <version>    Update sp1-prover version"
      echo "  --sp1-primitives <version> Update sp1-primitives version"
      echo "  --sp1-helper <version>    Update sp1-helper version"
      echo ""
      echo "RISC0 options:"
      echo "  --risc0-zkvm <version>    Update risc0-zkvm version"
      echo "  --risc0-platform <version> Update risc0-zkvm-platform version"
      echo "  --risc0-binfmt <version>  Update risc0-binfmt version"
      echo "  --bonsai-sdk <version>    Update bonsai-sdk version"
      echo ""
      echo "Zisk options:"
      echo "  --zisk-ref <git-ref>      Update zisk git reference (branch/tag/commit)"
      echo ""
      echo "Examples:"
      echo "  $0 --zkvm sp1 --sp1-sdk 5.0.3 --sp1-zkvm 5.0.1"
      echo "  $0 --zkvm risc0 --risc0-zkvm 2.3.0 --bonsai-sdk 1.5.0"
      echo "  $0 --zkvm zisk --zisk-ref main"
      echo "  $0 --zkvm sp1 --show-current"
      exit 0
      ;;
    *)
      echo "Unknown option: $1"
      echo "Use --help for usage information"
      exit 1
      ;;
  esac
done

if [ -z "$ZKVM" ]; then
  echo "Error: --zkvm parameter is required"
  echo "Use --help for usage information"
  exit 1
fi

# Function to extract version from Cargo.toml
extract_version() {
  local pattern="$1"
  local file="$2"
  grep "$pattern" "$file" | head -1 | sed 's/.*version = "\([^"]*\)".*/\1/' | sed 's/^=//'
}

# Function to show current versions
show_current_versions() {
  case $ZKVM in
    sp1)
      echo "Current SP1 versions:"
      echo "  sp1-sdk: $(extract_version 'sp1-sdk = ' Cargo.toml)"
      echo "  sp1-zkvm: $(extract_version 'sp1-zkvm = ' Cargo.toml)"
      echo "  sp1-prover: $(extract_version 'sp1-prover = ' Cargo.toml)"
      echo "  sp1-primitives: $(extract_version 'sp1-primitives = ' Cargo.toml)"
      echo "  sp1-helper: $(extract_version 'sp1-helper = ' Cargo.toml)"
      echo ""
      echo "In builder (provers/sp1/builder/Cargo.toml):"
      echo "  sp1-sdk: $(extract_version 'sp1-sdk = ' provers/sp1/builder/Cargo.toml)"
      echo ""
      echo "In guest (provers/sp1/guest/Cargo.toml):"
      echo "  sp1-zkvm: $(extract_version 'sp1-zkvm = ' provers/sp1/guest/Cargo.toml)"
      echo "  sp1-curves: $(extract_version 'sp1-curves = ' provers/sp1/guest/Cargo.toml)"
      ;;
    risc0)
      echo "Current RISC0 versions:"
      echo "  risc0-zkvm: $(extract_version 'risc0-zkvm = ' Cargo.toml)"
      echo "  bonsai-sdk: $(extract_version 'bonsai-sdk = ' Cargo.toml)"
      echo "  risc0-binfmt: $(extract_version 'risc0-binfmt = ' Cargo.toml)"
      echo ""
      echo "In guest (provers/risc0/guest/Cargo.toml):"
      echo "  risc0-zkvm: $(extract_version 'risc0-zkvm = ' provers/risc0/guest/Cargo.toml)"
      echo "  risc0-zkvm-platform: $(extract_version 'risc0-zkvm-platform = ' provers/risc0/guest/Cargo.toml)"
      ;;
    zisk)
      echo "Current Zisk version:"
      echo "  ziskos git: $(grep 'ziskos = ' provers/zisk/guest/Cargo.toml | sed 's/.*git = "\([^"]*\)".*/\1/')"
      local zisk_ref=$(grep 'ziskos = ' provers/zisk/guest/Cargo.toml)
      if [[ "$zisk_ref" == *"branch"* ]]; then
        echo "  branch: $(echo "$zisk_ref" | sed 's/.*branch = "\([^"]*\)".*/\1/')"
      elif [[ "$zisk_ref" == *"tag"* ]]; then
        echo "  tag: $(echo "$zisk_ref" | sed 's/.*tag = "\([^"]*\)".*/\1/')"
      elif [[ "$zisk_ref" == *"rev"* ]]; then
        echo "  commit: $(echo "$zisk_ref" | sed 's/.*rev = "\([^"]*\)".*/\1/')"
      fi
      ;;
  esac
}

# Show current versions if requested
if [ "$SHOW_CURRENT" = true ]; then
  show_current_versions
  exit 0
fi

# Check if any version was specified for the selected zkVM
case $ZKVM in
  sp1)
    if [ -z "$SP1_SDK_VERSION" ] && [ -z "$SP1_ZKVM_VERSION" ] && [ -z "$SP1_PROVER_VERSION" ] && [ -z "$SP1_PRIMITIVES_VERSION" ] && [ -z "$SP1_HELPER_VERSION" ]; then
      echo "Error: No SP1 version specified. Use --show-current to see current versions."
      echo "Available options: --sp1-sdk, --sp1-zkvm, --sp1-prover, --sp1-primitives, --sp1-helper"
      exit 1
    fi
    ;;
  risc0)
    if [ -z "$RISC0_ZKVM_VERSION" ] && [ -z "$RISC0_PLATFORM_VERSION" ] && [ -z "$RISC0_BINFMT_VERSION" ] && [ -z "$BONSAI_SDK_VERSION" ]; then
      echo "Error: No RISC0 version specified. Use --show-current to see current versions."
      echo "Available options: --risc0-zkvm, --risc0-platform, --risc0-binfmt, --bonsai-sdk"
      exit 1
    fi
    ;;
  zisk)
    if [ -z "$ZISK_GIT_REF" ]; then
      echo "Error: No Zisk reference specified. Use --show-current to see current versions."
      echo "Available options: --zisk-ref"
      exit 1
    fi
    ;;
esac

# Create backup of files before modification
echo "Creating backup of Cargo.toml files..."
cp Cargo.toml Cargo.toml.backup.$(date +%Y%m%d_%H%M%S)

# Update versions based on zkVM
case $ZKVM in
  sp1)
    echo "Updating SP1 versions..."
    
    # Update root Cargo.toml
    if [ -n "$SP1_SDK_VERSION" ]; then
      sed -i "s/sp1-sdk = { version = \"[^\"]*\"/sp1-sdk = { version = \"$SP1_SDK_VERSION\"/" Cargo.toml
      sed -i "s/sp1-sdk = \"[^\"]*\"/sp1-sdk = \"=$SP1_SDK_VERSION\"/" provers/sp1/builder/Cargo.toml
      echo "  ✓ Updated sp1-sdk to $SP1_SDK_VERSION"
    fi
    
    if [ -n "$SP1_ZKVM_VERSION" ]; then
      sed -i "s/sp1-zkvm = { version = \"[^\"]*\"/sp1-zkvm = { version = \"$SP1_ZKVM_VERSION\"/" Cargo.toml
      sed -i "s/sp1-zkvm = { version = \"[^\"]*\"/sp1-zkvm = { version = \"$SP1_ZKVM_VERSION\"/" provers/sp1/guest/Cargo.toml
      sed -i "s/sp1-curves = { version = \"[^\"]*\"/sp1-curves = { version = \"$SP1_ZKVM_VERSION\"/" provers/sp1/guest/Cargo.toml
      echo "  ✓ Updated sp1-zkvm and sp1-curves to $SP1_ZKVM_VERSION"
    fi
    
    if [ -n "$SP1_PROVER_VERSION" ]; then
      sed -i "s/sp1-prover = { version = \"[^\"]*\"/sp1-prover = { version = \"$SP1_PROVER_VERSION\"/" Cargo.toml
      echo "  ✓ Updated sp1-prover to $SP1_PROVER_VERSION"
    fi
    
    if [ -n "$SP1_PRIMITIVES_VERSION" ]; then
      sed -i "s/sp1-primitives = { version = \"[^\"]*\"/sp1-primitives = { version = \"$SP1_PRIMITIVES_VERSION\"/" Cargo.toml
      echo "  ✓ Updated sp1-primitives to $SP1_PRIMITIVES_VERSION"
    fi
    
    if [ -n "$SP1_HELPER_VERSION" ]; then
      sed -i "s/sp1-helper = { version = \"[^\"]*\"/sp1-helper = { version = \"$SP1_HELPER_VERSION\"/" Cargo.toml
      echo "  ✓ Updated sp1-helper to $SP1_HELPER_VERSION"
    fi
    
    # Update SP1 core-executor in builder if specified
    if [ -n "$SP1_ZKVM_VERSION" ]; then
      sed -i "s/sp1-core-executor = \"[^\"]*\"/sp1-core-executor = \"=$SP1_ZKVM_VERSION\"/" provers/sp1/builder/Cargo.toml 2>/dev/null || true
      echo "  ✓ Updated sp1-core-executor to $SP1_ZKVM_VERSION"
    fi
    
    echo "SP1 versions updated successfully!"
    ;;
    
  risc0)
    echo "Updating RISC0 versions..."
    
    if [ -n "$RISC0_ZKVM_VERSION" ]; then
      sed -i "s/risc0-zkvm = { version = \"[^\"]*\"/risc0-zkvm = { version = \"=$RISC0_ZKVM_VERSION\"/" Cargo.toml
      sed -i "s/risc0-zkvm = { version = \"[^\"]*\"/risc0-zkvm = { version = \"=$RISC0_ZKVM_VERSION\"/" provers/risc0/guest/Cargo.toml
      echo "  ✓ Updated risc0-zkvm to $RISC0_ZKVM_VERSION"
    fi
    
    if [ -n "$RISC0_PLATFORM_VERSION" ]; then
      sed -i "s/risc0-zkvm-platform = { version = \"[^\"]*\"/risc0-zkvm-platform = { version = \"=$RISC0_PLATFORM_VERSION\"/" provers/risc0/guest/Cargo.toml
      echo "  ✓ Updated risc0-zkvm-platform to $RISC0_PLATFORM_VERSION"
    fi
    
    if [ -n "$RISC0_BINFMT_VERSION" ]; then
      sed -i "s/risc0-binfmt = { version = \"[^\"]*\"/risc0-binfmt = { version = \"=$RISC0_BINFMT_VERSION\"/" Cargo.toml
      echo "  ✓ Updated risc0-binfmt to $RISC0_BINFMT_VERSION"
    fi
    
    if [ -n "$BONSAI_SDK_VERSION" ]; then
      sed -i "s/bonsai-sdk = { version = \"[^\"]*\"/bonsai-sdk = { version = \"=$BONSAI_SDK_VERSION\"/" Cargo.toml
      echo "  ✓ Updated bonsai-sdk to $BONSAI_SDK_VERSION"
    fi
    
    echo "RISC0 versions updated successfully!"
    ;;
    
  zisk)
    echo "Updating Zisk version..."
    
    if [ -n "$ZISK_GIT_REF" ]; then
      # Update git reference (could be branch, tag, or commit)
      if [[ "$ZISK_GIT_REF" =~ ^[0-9a-f]{40}$ ]]; then
        # It's a commit hash
        sed -i "s|ziskos = { git = \"[^\"]*\".*}|ziskos = { git = \"https://github.com/0xPolygonHermez/zisk.git\", rev = \"$ZISK_GIT_REF\" }|" provers/zisk/guest/Cargo.toml
        echo "  ✓ Updated ziskos to commit $ZISK_GIT_REF"
      elif [[ "$ZISK_GIT_REF" =~ ^v[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
        # It's a version tag
        sed -i "s|ziskos = { git = \"[^\"]*\".*}|ziskos = { git = \"https://github.com/0xPolygonHermez/zisk.git\", tag = \"$ZISK_GIT_REF\" }|" provers/zisk/guest/Cargo.toml
        echo "  ✓ Updated ziskos to tag $ZISK_GIT_REF"
      else
        # It's a branch
        sed -i "s|ziskos = { git = \"[^\"]*\".*}|ziskos = { git = \"https://github.com/0xPolygonHermez/zisk.git\", branch = \"$ZISK_GIT_REF\" }|" provers/zisk/guest/Cargo.toml
        echo "  ✓ Updated ziskos to branch $ZISK_GIT_REF"
      fi
    fi
    
    echo "Zisk version updated successfully!"
    ;;
    
  *)
    echo "Error: Unknown zkVM '$ZKVM'"
    exit 1
    ;;
esac

echo ""
echo "✅ Version update completed!"
echo ""
echo "Next steps:"
echo "1. Review the changes: git diff"
echo "2. Update lock files: cargo update"
echo "3. Test the build: TARGET=$ZKVM make build" 
echo "4. Commit the changes: git add -A && git commit -m 'Update $ZKVM versions'"
echo ""
echo "To see updated versions: $0 --zkvm $ZKVM --show-current"