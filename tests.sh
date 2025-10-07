#!/bin/bash

# Create test files and directories
mkdir -p test_dir
touch test_file.bin
touch test_dir/dest_file.bin

# Initialize counters
total_tests=0
passed_tests=0
failed_tests=0

# Color codes for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Function to test with given arguments and optional flags
test_case() {
    ((total_tests++))
    local description="$1"
    shift
    local expected_exit="$1"
    shift
    local args=("$@")

    ./target/debug/zap "${args[@]}" >/dev/null 2>&1
    exit_code=$?

    if [ "$exit_code" -eq "$expected_exit" ]; then
        echo -e "${GREEN}✓${NC} Test $total_tests: $description"
        ((passed_tests++))
    else
        echo -e "${RED}✗${NC} Test $total_tests: $description"
        echo -e "  Expected exit code $expected_exit, got $exit_code"
        echo -e "  Args: ${args[*]}"
        ((failed_tests++))
    fi
}

echo "========================================="
echo "Zap Local Test Suite"
echo "========================================="
echo ""

# ==========================================
# SECTION 1: Missing Files
# ==========================================
echo -e "${YELLOW}[1] Missing Files Tests${NC}"
test_case "Source file doesn't exist" 1 "nonexistent_file.bin" "test_dir/"
test_case "Destination directory doesn't exist (local)" 1 "test_file.bin" "nonexistent_dir/"

# ==========================================
# SECTION 2: Invalid Remote Format
# ==========================================
echo -e "\n${YELLOW}[2] Invalid Remote Format Tests${NC}"
test_case "Remote without colon" 1 "test_file.bin" "user@localhost"
test_case "Empty user with @" 1 "test_file.bin" "@localhost:path"
test_case "Empty host with @" 1 "test_file.bin" "user@:path"
test_case "Host ending with @" 1 "test_file.bin" "localhost@:path"
test_case "Only localhost (no colon or @)" 1 "test_file.bin" "localhost"
test_case "Colon but empty host" 1 "test_file.bin" ":path"

# ==========================================
# SECTION 3: Both Local or Both Remote
# ==========================================
echo -e "\n${YELLOW}[3] Invalid Source/Dest Combinations${NC}"
test_case "Both local paths" 1 "test_file.bin" "test_dir/"
test_case "Both remote paths" 1 "user@host1:/file" "user@host2:/path"

# ==========================================
# SECTION 4: IPv6 Address Parsing
# ==========================================
echo -e "\n${YELLOW}[4] IPv6 Address Format Tests${NC}"
test_case "IPv6 without brackets (invalid)" 1 "test_file.bin" "user@2001:db8::1:/path"
test_case "IPv6 with user@ prefix" 1 "test_file.bin" "user@[::1]:"
test_case "IPv6 without user@ prefix" 1 "test_file.bin" "[::1]:"
test_case "IPv6 malformed - no closing bracket" 1 "test_file.bin" "user@[2001:db8::1:/path"
test_case "IPv6 malformed - no opening bracket" 1 "test_file.bin" "user@2001:db8::1]:/path"

# ==========================================
# SECTION 5: Port Validation
# ==========================================
echo -e "\n${YELLOW}[5] Port Validation Tests${NC}"
test_case "Port 0 (invalid)" 1 -p 0 "test_file.bin" "user@localhost:"
test_case "Port 65536 (too high)" 1 -p 65536 "test_file.bin" "user@localhost:"
test_case "Port 99999 (too high)" 1 -p 99999 "test_file.bin" "user@localhost:"
test_case "Port -1 (negative)" 2 -p -1 "test_file.bin" "user@localhost:"
test_case "Port with letters" 1 -p abc "test_file.bin" "user@localhost:"

# ==========================================
# SECTION 6: Streams Validation
# ==========================================
echo -e "\n${YELLOW}[6] Streams Validation Tests${NC}"
test_case "Streams 0 (invalid)" 1 -s 0 "test_file.bin" "user@localhost:"
test_case "Streams negative" 2 -s -5 "test_file.bin" "user@localhost:"
test_case "Streams with letters" 1 -s abc "test_file.bin" "user@localhost:"

# ==========================================
# SECTION 7: Retries Validation
# ==========================================
echo -e "\n${YELLOW}[7] Retries Validation Tests${NC}"
test_case "Retries negative" 2 -r -1 "test_file.bin" "user@localhost:"
test_case "Retries with letters" 1 -r xyz "test_file.bin" "user@localhost:"

# ==========================================
# SECTION 8: Path Parsing Edge Cases
# ==========================================
echo -e "\n${YELLOW}[8] Path Parsing Edge Cases${NC}"
test_case "Multiple @ symbols" 1 "test_file.bin" "user@name@host:/path"
test_case "Multiple colons in host:path" 1 "test_file.bin" "host:path:extra"
test_case "Empty path after colon" 1 "test_file.bin" "user@host:"
test_case "Just @ symbol" 1 "test_file.bin" "@"
test_case "Just colon" 1 "test_file.bin" ":"

# ==========================================
# SECTION 9: Windows Path Detection (on non-Windows)
# ==========================================
if [[ "$OSTYPE" != "msys" && "$OSTYPE" != "win32" ]]; then
    echo -e "\n${YELLOW}[9] Windows Path Detection Tests (should treat as local)${NC}"
    # These would be treated as local Windows paths and fail because:
    # 1. On Unix, C:\ paths don't exist
    # 2. They're detected as local (not remote) so validation fails
    test_case "Windows C: drive path" 1 "C:\\Users\\file.txt" "test_dir/"
    test_case "Windows D: drive path" 1 "D:\\path\\file.txt" "test_dir/"
    test_case "Windows UNC path" 1 "\\\\server\\share\\file.txt" "test_dir/"
fi

# ==========================================
# SECTION 10: Special Characters in Paths
# ==========================================
echo -e "\n${YELLOW}[10] Special Characters Tests${NC}"
test_case "Space in remote path" 1 "test_file.bin" "user@host:/path with spaces"
test_case "Tab in remote path" 1 "test_file.bin" "user@host:/path	tab"
test_case "Null username" 1 "test_file.bin" "@host:/path"

# ==========================================
# SECTION 11: Mixed Valid/Invalid Arguments
# ==========================================
echo -e "\n${YELLOW}[11] Mixed Argument Tests${NC}"
test_case "Valid format but source missing + bad port" 1 -p 0 "nonexistent.bin" "user@localhost:"
test_case "Valid format but invalid streams" 2 -s -1 "test_file.bin" "user@localhost:"
test_case "Multiple invalid flags" 2 -p 99999 -s -1 -r -5 "test_file.bin" "user@localhost:"

# ==========================================
# SECTION 12: File vs Directory Validation
# ==========================================
echo -e "\n${YELLOW}[12] File/Directory Type Tests${NC}"
test_case "Source is directory not file" 1 "test_dir" "user@localhost:"
test_case "Local dest is file not directory" 1 "user@localhost:/file.bin" "test_file.bin"

# ==========================================
# SECTION 13: Empty Arguments
# ==========================================
echo -e "\n${YELLOW}[13] Empty/Missing Arguments Tests${NC}"
test_case "No arguments at all" 2
test_case "Only source, no dest" 2 "test_file.bin"
test_case "Empty string source" 1 "" "test_dir/"
test_case "Empty string dest" 1 "test_file.bin" ""

# ==========================================
# SECTION 14: Colon Edge Cases
# ==========================================
echo -e "\n${YELLOW}[14] Colon Position Edge Cases${NC}"
test_case "Leading colon" 1 "test_file.bin" ":host/path"
test_case "Trailing colon only" 1 "test_file.bin" "host:"
test_case "Double colon (not IPv6)" 1 "test_file.bin" "host::/path"
test_case "Colon in middle of path" 1 "test_file.bin" "user@host:/path:subpath"

# ==========================================
# SECTION 15: Hostname Validation
# ==========================================
echo -e "\n${YELLOW}[15] Hostname Edge Cases${NC}"
test_case "Hostname with spaces" 1 "test_file.bin" "user@host name:/path"
test_case "Very long hostname" 1 "test_file.bin" "user@$(printf 'a%.0s' {1..300}):/path"
test_case "Hostname with special chars" 1 "test_file.bin" "user@host!@#:/path"

# ==========================================
# SECTION 16: Username Validation
# ==========================================
echo -e "\n${YELLOW}[16] Username Edge Cases${NC}"
test_case "Username with spaces" 1 "test_file.bin" "user name@host:/path"
test_case "Username with @" 1 "test_file.bin" "user@email.com@host:/path"
test_case "Very long username" 1 "test_file.bin" "$(printf 'u%.0s' {1..300})@host:/path"

# ==========================================
# SECTION 17: Quiet Mode (should still fail validation)
# ==========================================
echo -e "\n${YELLOW}[17] Quiet Mode Tests${NC}"
test_case "Quiet mode with invalid source" 1 -q "nonexistent.bin" "user@localhost:"
test_case "Quiet mode with invalid format" 1 -q "test_file.bin" "user@localhost"
test_case "Quiet mode with bad port" 1 -q -p 99999 "test_file.bin" "user@localhost:"

# ==========================================
# SECTION 18: SSH Key Path (file doesn't exist)
# ==========================================
echo -e "\n${YELLOW}[18] SSH Key Path Tests${NC}"
test_case "Non-existent SSH key" 1 -i "/nonexistent/key" "test_file.bin" "user@localhost:"
test_case "SSH key is directory" 1 -i "/tmp" "test_file.bin" "user@localhost:"

# ==========================================
# Clean up
# ==========================================
rm -rf test_dir
rm -f test_file.bin

# ==========================================
# Output test summary
# ==========================================
echo ""
echo "========================================="
echo "Test Summary"
echo "========================================="
echo -e "Total tests:  ${YELLOW}$total_tests${NC}"
echo -e "Passed:       ${GREEN}$passed_tests${NC}"
echo -e "Failed:       ${RED}$failed_tests${NC}"
echo ""

if [ "$failed_tests" -eq 0 ]; then
    echo -e "${GREEN}All tests passed!${NC}"
    exit 0
else
    echo -e "${RED}Some tests failed!${NC}"
    exit 1
fi

