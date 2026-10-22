"""Small simulation CLI check; requires the existing isolated Podman image/artifact."""
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parents[1]
WRAPPER = ["sh", str(ROOT / "scripts/run-desktop-driver-simulated.sh")]
commands = (ROOT / "tests/desktop-driver.jsonl").read_text().splitlines()


def run(lines, expected_code=0):
    result = subprocess.run(WRAPPER, input="\n".join(lines) + "\n", text=True,
                            capture_output=True, cwd=ROOT, timeout=30)
    assert result.returncode == expected_code, result.stderr
    rows = [json.loads(line) for line in result.stdout.splitlines()]
    assert rows[0]["real_input"] is False
    assert rows[0]["kde_connection"] is False
    assert rows[0]["vr_runtime"] is False
    final = rows[-1]
    assert not final["active"] and not final["held_buttons"]
    assert not final["held_keys"] and final["modifiers"] == 0
    return rows


rows = run(commands)
assert not rows[1]["accepted"] and not rows[1]["events"]
assert not rows[4]["active"]  # Hover does not activate.
assert not rows[5]["events"] and not rows[6]["events"]  # First click consumed.
assert rows[6]["active"] and rows[6]["keyboard_route"] == "PhysicalScreen"
assert {"mouse": [-400.0, 400.0]} in rows[7]["events"]
assert {"button": 273, "down": False} in rows[10]["events"]
assert {"wheel": [0.0, 30.0]} in rows[11]["events"]
assert rows[14]["held_buttons"] and rows[14]["held_keys"] == ["A"]
assert not rows[15]["held_buttons"] and not rows[15]["held_keys"]
assert not rows[15]["active"] and rows[15]["modifiers"] == 0
assert not rows[17]["accepted"]  # Recovery cannot silently resume typing.

# Terminate with simulated mouse/key/modifiers held; cleanup must flush releases.
for suffix, code in (([], 0), (["not JSON"], 1)):
    final = run(commands[:14] + suffix, code)[-1]
    assert {"button": 272, "down": False} in final["events"]
    assert {"key": "A", "down": False} in final["events"]
    assert {"modifiers": 0} in final["events"]

result = subprocess.run(WRAPPER + ["--replace"], capture_output=True, text=True)
assert result.returncode == 2  # Wrapper cannot pass runtime arguments.
print("PASS: simulation trace, EOF/error release, and wrapper argument guard")
