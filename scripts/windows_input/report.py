"""Portable case catalogue and strict evidence verdicts for the local Windows gauntlet.

This is not a host simulator. A report cannot turn absent Windows evidence into a pass.
"""
import argparse
import json
import re
from pathlib import Path

WIDTHS = [80, 119, 120, 121, 132, 160, 240]
HEIGHTS = [24, 50]
MODES = ["native", "legacy", "mok2", "kitty"]


def hex_of(text):
    return text.encode("utf-8").hex()


def catalogue():
    cases = []

    def key(name, vk, text, modifiers=(), enhanced=None, legacy=None):
        mask = (1 if 16 in modifiers else 0) | (2 if 18 in modifiers else 0) | (4 if 17 in modifiers else 0)
        unicode = (vk + 32 if 65 <= vk <= 90 else vk if vk in (8, 9, 13, 27) else 0)
        if 65 <= vk <= 90 and 16 in modifiers:
            unicode = vk
        if 17 in modifiers:
            unicode = vk - 64 if 65 <= vk <= 90 else 10 if vk == 13 else 127 if vk == 8 else 0
        expected = {"native": {"vk": vk, "modifiers": mask, "unicode": unicode, "modifier_keys": list(modifiers)}}
        if text is not None:
            expected.update({mode: {"hex": [hex_of(text)]} for mode in MODES[1:]})
        if legacy is not None:
            expected["legacy"] = {"hex": [hex_of(legacy)]}
        if enhanced is not None:
            expected["mok2"] = {"hex": [hex_of(f"\x1b[27;{mask + 1};{enhanced}~")]}
            expected["kitty"] = {"hex": [hex_of(f"\x1b[{enhanced};{mask + 1}u")]}
        cases.append(dict(id=name, kind="key", chords=[[*modifiers, vk]], expected=expected))

    key("letter-a", 65, "a")
    key("shift-letter", 65, "A", (16,))
    key("enter", 13, "\r")
    key("shift-enter", 13, None, (16,), 13)
    key("ctrl-enter", 13, None, (17,), 13)
    key("ctrl-shift-enter", 13, None, (17, 16), 13)
    key("alt-enter", 13, None, (18,), 13, "\x1b\r")
    key("tab", 9, "\t")
    key("shift-tab", 9, "\x1b[Z", (16,))
    key("backspace", 8, "\x7f")
    key("ctrl-backspace", 8, None, (17,), 127, "\x08")
    key("alt-backspace", 8, None, (18,), 127, "\x1b\x7f")
    key("escape", 27, "\x1b")
    # Kitty's disambiguation flag encodes Escape distinctly.
    cases[-1]["expected"]["kitty"] = {"hex": [hex_of("\x1b[27u")]}
    for name, vk, seq in [("up", 38, "A"), ("down", 40, "B"), ("right", 39, "C"), ("left", 37, "D"), ("home", 36, "H"), ("end", 35, "F")]:
        for prefix, modifiers, modifier in [("", (), 1), ("shift-", (16,), 2), ("ctrl-", (17,), 5), ("ctrl-shift-", (17, 16), 6)]:
            key(prefix + name, vk, "\x1b[" + ("" if modifier == 1 else f"1;{modifier}") + seq, modifiers)
    for name, vk, code in [("insert", 45, 2), ("delete", 46, 3), ("page-up", 33, 5), ("page-down", 34, 6)]:
        key(name, vk, f"\x1b[{code}~")
    for letter in "acdj lmqsu vz".replace(" ", ""):
        key("ctrl-" + letter, ord(letter.upper()), None, (17,), ord(letter), chr(ord(letter) - 96))
    key("alt-v", 86, None, (18,), ord("v"), "\x1bv")
    for name, text in [
        ("paste-lf", "line 1\nline 2"),
        ("paste-crlf", "line 1\r\nline 2\r\n"),
        ("paste-cr", "line 1\rline 2"),
        ("paste-whitespace", "  one\t\n\n two  \n"),
        ("paste-unicode", "é e\u0301 日本語 😀\nnext"),
        ("paste-escape-looking", "literal [200~ and \\x1b[31m\nend"),
        ("paste-burst", "".join(f"line {i:04d} abcdefghijklmnopqrstuvwxyz\n" for i in range(200))),
    ]:
        cases.append(dict(id=name, kind="paste", text=text, expected={mode: {"paste": text} for mode in MODES[1:]}))
    for name, prompt, text in [
        ("dead-acute", "With US-International active, type acute then e, once; no Enter.", "é"),
        ("dead-grave", "With US-International active, type grave then e, once; no Enter.", "è"),
        ("dead-circumflex", "With US-International active, type circumflex then e, once; no Enter.", "ê"),
        ("dead-tilde", "With US-International active, type tilde then n, once; no Enter.", "ñ"),
        ("dead-diaeresis", "With US-International active, type diaeresis then u, once; no Enter.", "ü"),
        ("dead-space", "With US-International active, type acute then Space, once; no Enter.", "'"),
        ("altgr-euro", "With your declared euro-producing AltGr layout, type € using its AltGr chord, once; no Enter.", "€"),
        ("ime-commit", "Using your declared IME, compose and commit 日本語 (not paste); do not submit the prompt.", "日本語"),
    ]:
        cases.append(dict(id=name, kind="manual", prompt=prompt, expected={mode: {"hex": [hex_of(text)]} for mode in MODES[1:]}))
    # Explicit qualification tasks, never auto-passed by key/byte checks.
    for name, prompt in [
        ("dead-cancel-repeat", "Qualify dead-key cancellation, repeated accent and non-composing next character against the direct-host baseline."),
        ("ime-cancel", "Qualify IME cancel and partial composition without committed/duplicate text."),
        ("native-repeat-release", "Qualify held-key repeat, release identity, and modifier interleaving using native records."),
        ("locks-keypad", "Qualify CapsLock/NumLock, keypad Enter/operators/decimal and restore lock states."),
        ("mouse-right-edge", "Click/drag/wheel at a marked pane column above 120; compare actual report coordinates and visual target."),
        ("wrap-rendering", "Verify the displayed ruler and wrapped multiline text at both window heights and every observed width."),
        ("capture-refresh", "Toggle mouse capture/config reload, refocus, detach/reattach; repeat paste and Shift+Enter."),
        ("setup-recovery", "Inject a recoverable setup failure and late VT activation; verify mode restoration and the same input sentinels."),
        ("clipboard-nontext", "Qualify supported image/file clipboard integrations separately; do not infer from text paste."),
    ]:
        cases.append(dict(id=name, kind="qualification", prompt=prompt, expected={}))
    return dict(schema=1, widths=WIDTHS, heights=HEIGHTS, modes=MODES, cases=cases)


def verdict(case, mode, evidence):
    """The full capture, not a matching prefix, is the primary assertion."""
    if evidence.get("status") in {"not_run", "unsupported", "inconclusive"}:
        return evidence["status"], evidence.get("reason", "No qualifying observation")
    expected = case["expected"].get(mode)
    if expected is None:
        return "not_run", "No automatic oracle for this case/profile; requires qualification"
    if not evidence.get("ready") or not evidence.get("focus_verified") or not evidence.get("complete"):
        return "inconclusive", "Missing readiness, focus, or complete capture"
    if evidence.get("error"):
        return "inconclusive", evidence["error"]
    if evidence.get("outer_geometry", [])[:2] != [evidence.get("width"), evidence.get("height")]:
        return "inconclusive", "Requested geometry was not observed"
    if len(evidence.get("pane_geometry", [])) < 2 or min(evidence["pane_geometry"][:2]) <= 0:
        return "inconclusive", "Missing actual pane dimensions"
    if evidence.get("final_pane_geometry", [])[:2] != evidence["pane_geometry"][:2]:
        return "inconclusive", "Pane geometry changed during capture"
    if evidence.get("final_outer_geometry", [])[:2] != evidence["outer_geometry"][:2]:
        return "inconclusive", "Outer geometry changed during capture"
    if "vk" in expected:
        records = evidence.get("records")
        scans = evidence.get("scans", [])
        chord = case["chords"][0]
        if not isinstance(records, list):
            return "inconclusive", "Malformed native record"
        if not isinstance(scans, list) or len(scans) != len(chord) or not all(type(scan) is int for scan in scans):
            return "inconclusive", "Missing injected scan-code evidence"
        expected_scans = dict(zip(chord, scans))
        aliases = {160: 16, 161: 16, 162: 17, 163: 17, 164: 18, 165: 18}
        held = set()
        keys = []
        for record in records:
            if not isinstance(record, list) or len(record) != 7 or not all(type(field) is int for field in record):
                return "inconclusive", "Malformed native record"
            if record[0] in (4, 16):  # Resize/focus notifications carry no typed text.
                continue
            if record[0] != 1:
                return "fail", "Unexpected non-key native input"
            vk = aliases.get(record[3], record[3])
            if vk not in (16, 17, 18):
                keys.append(record)
                continue
            if vk not in expected["modifier_keys"] or record[5] != 0 or record[2] != 1 or record[4] != expected_scans[vk]:
                return "fail", "Unexpected/corrupted modifier record"
            if record[1] == 1 and vk not in held:
                held.add(vk)
            elif record[1] == 0 and vk in held:
                held.remove(vk)
            else:
                return "fail", "Unbalanced modifier sequence"
        if held:
            return "fail", "Modifier release missing"
        if len(keys) != 2 or [r[1] for r in keys] != [1, 0]:
            return "fail", "Expected exactly one non-modifier down/up pair"
        for record in keys:
            control = record[6]
            modifiers = bool(control & 16) + 2 * bool(control & 3) + 4 * bool(control & 12)
            if (record[3] != expected["vk"] or modifiers != expected["modifiers"] or record[2] != 1
                    or record[5] != expected["unicode"] or record[4] != expected_scans[expected["vk"]]):
                return "fail", "Native identity/modifiers/repeat differ"
        return "pass", "Native down/up pair matches"
    try:
        raw = bytes.fromhex(evidence["hex"])
    except (KeyError, ValueError, TypeError):
        return "inconclusive", "Missing or malformed raw bytes"
    if "hex" in expected:
        return ("pass", "Exact bytes match") if raw.hex() in expected["hex"] else ("fail", "Bytes differ (including any duplicates/trailing input)")
    if not raw.startswith(b"\x1b[200~") or not raw.endswith(b"\x1b[201~"):
        return "fail", "Missing bracketed-paste envelope; newlines may be key events"
    try:
        actual = raw[6:-6].decode("utf-8")
    except UnicodeDecodeError:
        return "fail", "Paste is not intact UTF-8"
    # Line-ending conversion is expected on Windows; all other payload bytes matter.
    normalize = lambda text: text.replace("\r\n", "\n").replace("\r", "\n")
    return ("pass", "One complete paste matches") if normalize(actual) == normalize(expected["paste"]) else ("fail", "Paste payload differs")


def known_host_gap(observation, run):
    """A narrowly observed host limitation, never a blanket version exemption."""
    value = run.get("terminal_version", "")
    version = re.match(r"^1\.24(?:\.|$)", value) if isinstance(value, str) else None
    return (version is not None and observation.get("path") == "direct"
            and observation.get("mode") == "mok2" and observation.get("case") == "shift-enter"
            and observation.get("hex") == "0d")


def channel_identity_errors(hosts):
    """Compare both launcher identities and the processes actually activated."""
    identities = {}
    for host in hosts:
        values = identities.setdefault(host.get("channel"), set())
        for field, kind in (("launcher_identity", "file"), ("installation_identity", "installation")):
            if host.get(field):
                values.add((kind, host[field]))
        for run in host.get("runs", []):
            for field, kind in (("image_identity", "file"), ("installation_identity", "installation"), ("process_identity", "process")):
                if run.get(field):
                    values.add((kind, run[field]))
    return ["Stable and Preview share a Terminal executable, installation, or process identity"] if identities.get("stable", set()) & identities.get("preview", set()) else []


def summarize(document):
    matrix = catalogue()
    cases = {case["id"]: case for case in matrix["cases"]}
    rows = []
    seen = set()
    captures = set()
    for observation in document.get("observations", []):
        identity = tuple(observation.get(k) for k in ("host", "path", "mode", "phase", "width", "height", "case"))
        if identity in seen:
            raise ValueError(f"Duplicate observation identity: {identity}")
        seen.add(identity)
        case = cases[observation["case"]]
        status, reason = verdict(case, observation["mode"], observation)
        scope = "direct_host" if observation.get("path") == "direct" else "through_herdr_not_yet_attributed"
        if status in ("pass", "fail"):
            bound = [run for host in document.get("hosts", []) if host.get("channel") == observation.get("host")
                     for run in host.get("runs", [])
                     if run.get("nonce") == observation.get("nonce") and run.get("path") == observation.get("path")
                     and run.get("mode") == observation.get("mode") and run.get("pid", 0) > 0 and run.get("hwnd", 0) != 0
                     and run.get("elevated") is False and run.get("image_identity") and run.get("installation_identity")]
            capture = observation.get("capture_id")
            if len(bound) != 1 or document.get("controller_elevated") is not False or not capture or capture in captures:
                status, reason = "inconclusive", "Missing non-elevated owned-run binding or fresh capture identity"
            else:
                captures.add(capture)
                if status == "fail" and known_host_gap(observation, bound[0]):
                    status, reason = "unsupported", "Observed WT 1.24 direct-host mOK Shift+Enter→CR limitation; not a Herdr regression"
        rows.append({**observation, "status": status, "reason": reason, "failure_scope": scope})
    counts = {status: sum(r["status"] == status for r in rows) for status in ("pass", "fail", "not_run", "unsupported", "inconclusive")}
    # No run may claim all-green just because it produced zero/missing observations.
    planned = set()
    geometries = [(120, 30, True)] + [(w, h, False) for h in document.get("heights", HEIGHTS) for w in document.get("widths", WIDTHS)] + [(80, 30, False)]
    for host in ("stable", "preview"):
        for path in ("direct", "herdr"):
            for mode in document.get("modes", MODES):
                for phase, (width, height, full) in enumerate(geometries, 1):
                    for case_id in cases if full else ("letter-a", "shift-enter", "paste-lf"):
                        planned.add((host, path, mode, phase, width, height, case_id))
    missing = planned - seen
    if seen - planned:
        raise ValueError("Observations outside the declared run matrix")
    hosts = document.get("hosts", [])
    errors = list(document.get("errors", [])) + channel_identity_errors(hosts)
    complete = (bool(rows) and not missing and {h.get("channel") for h in hosts} == {"stable", "preview"}
                and all(h.get("runs") for h in hosts)
                and not errors and not document.get("cleanup_errors")
                and all(r["status"] == "pass" for r in rows))
    return {**document, "errors": errors, "observations": rows, "counts": counts, "coverage_missing": len(missing), "observed_checks_passed": complete,
            "native_qualification": "Required; this report is not a full Windows support certificate"}


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("command", choices=["matrix", "report"])
    parser.add_argument("--input", type=Path)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    result = catalogue() if args.command == "matrix" else summarize(json.loads(args.input.read_text(encoding="utf-8-sig")))
    args.output.write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    if args.command == "report":
        print(json.dumps({**result["counts"], "coverage_missing": result["coverage_missing"]}))
        return 1 if result["counts"]["fail"] or result.get("errors") or result.get("cleanup_errors") else 2 if not result["observed_checks_passed"] else 0
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
