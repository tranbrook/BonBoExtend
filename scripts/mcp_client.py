#!/usr/bin/env python3
"""
BonBo MCP Client Library — shared retry/timeout wrapper for all scripts.

Provides:
- MCPClient: subprocess-based MCP tool caller with retry + backoff
- call_mcp_with_retry: convenience wrapper for single-shot calls
- safe_float, safe_int: safe parsing helpers
"""

import json
import os
import subprocess
import sys
import time
import threading


# Default path to MCP binary
DEFAULT_MCP_BIN = os.path.join(
    os.path.dirname(os.path.dirname(os.path.abspath(__file__))),
    "target/release/bonbo-extend-mcp",
)


class MCPClient:
    """Thread-safe MCP client with retry logic and error recovery."""

    def __init__(self, bin_path=None, market_type="futures"):
        self.bin_path = bin_path or DEFAULT_MCP_BIN
        if not os.path.isfile(self.bin_path):
            raise FileNotFoundError(
                f"MCP binary not found: {self.bin_path}\n"
                f"Run: cargo build --release -p bonbo-extend-mcp"
            )
        self.market_type = market_type
        self._lock = threading.Lock()
        self._seq = 0
        self._call_count = 0
        self._error_count = 0

    @property
    def stats(self):
        """Return call statistics."""
        return {"calls": self._call_count, "errors": self._error_count}

    def call(self, tool, args=None, timeout=45, max_retries=3):
        """Call an MCP tool with retry logic and proper error handling.

        Args:
            tool: MCP tool name (e.g., 'technical_analysis')
            args: Tool arguments dict
            timeout: Per-attempt timeout in seconds
            max_retries: Max retry attempts on transient errors

        Returns:
            Tool result text, or empty string after all retries exhausted.
        """
        if args is None:
            args = {}

        last_error = None
        for attempt in range(max_retries):
            try:
                result = self._call_once(tool, args, timeout)
                self._call_count += 1
                if result:
                    return result
                # Empty result might be transient
                if attempt < max_retries - 1:
                    time.sleep(0.5 * (attempt + 1))
                    continue
            except subprocess.TimeoutExpired:
                last_error = f"Timeout after {timeout}s"
                print(
                    f"  ⚠️ MCP timeout on {tool} (attempt {attempt+1}/{max_retries})",
                    file=sys.stderr,
                )
                if attempt < max_retries - 1:
                    time.sleep(1.0 * (attempt + 1))
                    continue
            except FileNotFoundError as e:
                print(f"  ❌ MCP binary not found: {e}", file=sys.stderr)
                self._error_count += 1
                return ""
            except Exception as e:
                err_msg = str(e)
                if "not found" in err_msg.lower() or "permission" in err_msg.lower():
                    print(f"  ❌ MCP error (non-retriable): {err_msg}", file=sys.stderr)
                    self._error_count += 1
                    return ""
                last_error = err_msg
                print(
                    f"  ⚠️ MCP error on {tool}: {err_msg} "
                    f"(attempt {attempt+1}/{max_retries})",
                    file=sys.stderr,
                )
                if attempt < max_retries - 1:
                    time.sleep(0.5 * (attempt + 1))
                    continue

        self._call_count += 1
        self._error_count += 1
        if last_error:
            print(f"  ❌ MCP failed after {max_retries} retries: {last_error}", file=sys.stderr)
        return ""

    def _call_once(self, tool, args, timeout):
        """Single MCP subprocess call."""
        with self._lock:
            self._seq += 1
            init_req = json.dumps(
                {
                    "jsonrpc": "2.0",
                    "method": "initialize",
                    "params": {
                        "protocolVersion": "2024-11-05",
                        "capabilities": {},
                        "clientInfo": {"name": "bonbo-script", "version": "1.0"},
                    },
                    "id": "0",
                }
            )
            call_req = json.dumps(
                {
                    "jsonrpc": "2.0",
                    "method": "tools/call",
                    "params": {"name": tool, "arguments": args},
                    "id": str(self._seq),
                }
            )
            stdin_data = init_req + "\n" + call_req + "\n"

            env = os.environ.copy()
            env["BINANCE_MARKET_TYPE"] = self.market_type

            p = subprocess.run(
                [self.bin_path],
                input=stdin_data,
                capture_output=True,
                text=True,
                timeout=timeout,
                env=env,
            )

            for line in p.stdout.strip().split("\n"):
                try:
                    r = json.loads(line)
                    if "result" in r and "content" in r["result"]:
                        for c in r["result"]["content"]:
                            if c.get("type") == "text":
                                return c["text"]
                except json.JSONDecodeError:
                    continue

            if p.returncode != 0 and p.stderr:
                stderr_first = p.stderr.strip().split("\n")[0]
                if stderr_first:
                    raise RuntimeError(f"MCP server error: {stderr_first}")
            return ""


def call_mcp_with_retry(tool, args=None, timeout=45, max_retries=3, market_type="futures"):
    """Convenience function for single-shot MCP calls with retry.

    Creates a temporary client, calls the tool, and returns the result.
    For batch operations, create an MCPClient instance instead.
    """
    client = MCPClient(market_type=market_type)
    return client.call(tool, args, timeout, max_retries)


def safe_float(text, pattern=r"([0-9.]+)", default=0.0):
    """Safely extract a float from text using regex."""
    import re

    m = re.search(pattern, text)
    return float(m.group(1)) if m else default


def safe_int(text, pattern=r"([0-9]+)", default=0):
    """Safely extract an int from text using regex."""
    import re

    m = re.search(pattern, text)
    return int(m.group(1)) if m else default
