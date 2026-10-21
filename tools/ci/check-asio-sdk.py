#!/usr/bin/env python3
"""Compare the private Rust ABI against the digest-pinned ASIO SDK, never extract it.

Run with python3 -I from the repository, with --archive in an empty scratch folder.
Downloaded SDK contents are data only. No preprocessor, compiler or eval is used.
"""
from __future__ import annotations
import argparse
import ast
import hashlib
from pathlib import Path
import re
import zipfile

URL = "https://download.steinberg.net/sdk_downloads/ASIO-SDK_2.3.4_2025-10-15.zip"
SHA256 = "d5ebf0c20dd2c5f43771fd0c1418f4b361bf52434ee670097cfa6b3a335e2eca"
MAX_BYTES = 32 * 1024 * 1024


def uncomment(text: str) -> str:
    return re.sub(r"/\*.*?\*/|//[^\n]*", "", text, flags=re.S)


def integer(expression: str, names: dict[str, int]) -> int:
    def visit(node: ast.AST) -> int:
        if isinstance(node, ast.Constant) and type(node.value) is int:
            return node.value
        if isinstance(node, ast.Name) and node.id in names:
            return names[node.id]
        if isinstance(node, ast.UnaryOp) and isinstance(node.op, ast.USub):
            return -visit(node.operand)
        if isinstance(node, ast.BinOp):
            left, right = visit(node.left), visit(node.right)
            if isinstance(node.op, ast.LShift) and 0 <= right <= 63:
                return left << right
            if isinstance(node.op, ast.BitOr):
                return left | right
            if isinstance(node.op, ast.Add):
                return left + right
        raise ValueError(f"unsupported integer expression: {expression}")
    return visit(ast.parse(expression.strip(), mode="eval").body)


def enums(header: str) -> dict[str, int]:
    values: dict[str, int] = {}
    for body in re.findall(r"\benum\b[^{};]*\{([^{}]*)\}", uncomment(header), re.S):
        next_value = 0
        for member in body.split(","):
            member = member.strip()
            if not member:
                continue
            parts = member.split("=", 1)
            name = parts[0].strip()
            if not re.fullmatch(r"[A-Za-z_]\w*", name):
                raise ValueError(f"invalid enum member: {name}")
            value = integer(parts[1], values) if len(parts) == 2 else next_value
            values[name] = value
            next_value = value + 1
    return values


def compare(header: str, interface: str, rust: str) -> None:
    expected = enums(header)
    actual = {name: integer(value, {}) for name, value in re.findall(
        r"pub const (\w+): i32\s*=\s*([^;]+);", uncomment(rust))}
    if not expected or actual != expected:
        differences = [f"{key}: SDK={expected.get(key)} Rust={actual.get(key)}"
                       for key in sorted(expected.keys() | actual.keys())
                       if expected.get(key) != actual.get(key)]
        raise ValueError("enum mismatch: " + "; ".join(differences))
    cpp_methods = re.findall(r"virtual\s+\w+\s+(\w+)\s*\(", uncomment(interface))
    body = re.search(r"pub struct AsioVtable\s*\{(.*?)\n\}", uncomment(rust), re.S)
    rust_methods = re.findall(r"pub (\w+):\s*unsafe extern", body.group(1) if body else "")
    if not cpp_methods or cpp_methods != rust_methods:
        raise ValueError(f"IASIO method order mismatch: SDK={cpp_methods} Rust={rust_methods}")


def verify(archive: Path, rust_path: Path, digest: str = SHA256) -> None:
    if archive.stat().st_size > MAX_BYTES:
        raise ValueError("archive exceeds size limit")
    data = archive.read_bytes()
    if hashlib.sha256(data).hexdigest() != digest:
        raise ValueError("SDK SHA-256 mismatch")
    with zipfile.ZipFile(archive) as sdk:
        def read(member: str) -> str:
            info = sdk.getinfo(member)
            if info.file_size > MAX_BYTES:
                raise ValueError("member exceeds size limit")
            return sdk.read(info).decode("utf-8-sig")
        compare(read("ASIOSDK/common/asio.h"), read("ASIOSDK/common/iasiodrv.h"),
                rust_path.read_text(encoding="utf-8"))


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--archive", type=Path, required=True)
    parser.add_argument("--sha256", required=True)
    parser.add_argument("--rust", type=Path, default=Path("crates/asio-sys/src/sdk.rs"))
    args = parser.parse_args()
    if not re.fullmatch(r"[0-9a-fA-F]{64}", args.sha256):
        parser.error("--sha256 must contain 64 hexadecimal characters")
    verify(args.archive, args.rust, args.sha256.lower())
    print("ASIO SDK digest, enum values and IASIO method order verified")


if __name__ == "__main__":
    main()
