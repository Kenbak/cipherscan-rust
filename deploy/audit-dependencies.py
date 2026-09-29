"""Audit the lockfile; permit RSA only while absent from every compiled target."""
import subprocess

graph = subprocess.check_output(
    ["cargo", "tree", "--locked", "--target", "all", "--edges", "normal,build",
     "--prefix", "none", "--format", "{p}"], text=True
)
if any(line.startswith("rsa v") for line in graph.splitlines()):
    raise SystemExit("RSA entered the compiled graph; remove it or resolve RUSTSEC-2023-0071")
# Cargo locks SQLx's disabled optional MySQL dependencies, including RSA.
# This PostgreSQL-only binary disables SQLx default features. Never silently
# carry this exception into a build that enables RSA; the graph check above fails.
subprocess.run(["cargo", "audit", "--deny", "unsound", "--ignore", "RUSTSEC-2023-0071"], check=True)
