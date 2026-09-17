"""Portable tests of the gauntlet's oracle, not Windows input qualification."""
import copy
import unittest
from scripts.windows_input.report import catalogue, channel_identity_errors, summarize, verdict


class WindowsInputGauntletTests(unittest.TestCase):
    def setUp(self):
        self.matrix = catalogue()
        self.cases = {case["id"]: case for case in self.matrix["cases"]}
        self.evidence = dict(ready=True, focus_verified=True, complete=True, width=121, height=24,
                             outer_geometry=[121, 24, 0x298], pane_geometry=[100, 20, 0x200],
                             final_outer_geometry=[121, 24, 0x298], final_pane_geometry=[100, 20, 0x200])

    def test_catalogue_has_unique_cases_and_geometry_boundaries(self):
        self.assertEqual(len(self.cases), len(self.matrix["cases"]))
        self.assertEqual(self.matrix["widths"], [80, 119, 120, 121, 132, 160, 240])
        self.assertEqual(self.matrix["heights"], [24, 50])
        for case in self.cases.values():
            self.assertTrue(case["id"])
            self.assertLessEqual(set(case["expected"]), set(self.matrix["modes"]))
            for expected in case["expected"].values():
                for value in expected.get("hex", []):
                    self.assertTrue(bytes.fromhex(value))

    def test_shift_enter_cannot_pass_with_plain_enter_or_trailing_duplicates(self):
        case = self.cases["shift-enter"]
        correct = "1b5b32373b323b31337e"
        for value, status in [("0d", "fail"), (correct, "pass"), (correct + "0d", "fail")]:
            self.assertEqual(verdict(case, "mok2", {**self.evidence, "hex": value})[0], status)
        self.assertEqual(verdict(case, "legacy", self.evidence)[0], "not_run")

    def test_focus_readiness_and_actual_geometry_are_required(self):
        case = self.cases["letter-a"]
        good = {**self.evidence, "hex": "61"}
        for field in ["ready", "focus_verified", "complete", "pane_geometry", "outer_geometry", "final_pane_geometry", "final_outer_geometry"]:
            value = copy.deepcopy(good)
            value.pop(field)
            self.assertEqual(verdict(case, "legacy", value)[0], "inconclusive", field)
        self.assertEqual(verdict(case, "legacy", {**good, "outer_geometry": [120, 24]})[0], "inconclusive")

    def test_native_modifiers_release_and_repeat_are_not_discarded(self):
        # type, down, repeat, vk, scan, Unicode, control-state
        self.evidence["scans"] = [42, 28]
        records = [[1, 1, 1, 13, 28, 13, 16], [1, 0, 1, 13, 28, 13, 16]]
        case = self.cases["shift-enter"]
        self.assertEqual(verdict(case, "native", {**self.evidence, "records": records})[0], "pass")
        for index, value in [(6, 0), (2, 2), (3, 10), (4, 0), (5, 122)]:
            bad = copy.deepcopy(records)
            bad[0][index] = value
            self.assertEqual(verdict(case, "native", {**self.evidence, "records": bad})[0], "fail")
        self.assertEqual(verdict(case, "native", {**self.evidence, "records": records[:1]})[0], "fail")
        extra = [[1, 1, 1, 16, 42, 0, 16]] + records
        self.assertEqual(verdict(case, "native", {**self.evidence, "records": extra})[0], "fail")
        balanced = extra + [[1, 0, 1, 16, 42, 0, 0]]
        self.assertEqual(verdict(case, "native", {**self.evidence, "records": balanced})[0], "pass")

    def test_paste_requires_one_envelope_and_complete_unicode_payload(self):
        case = self.cases["paste-unicode"]
        payload = case["text"].replace("\n", "\r\n").encode()
        framed = b"\x1b[200~" + payload + b"\x1b[201~"
        for data, expected in [(framed, "pass"), (payload, "fail"), (framed * 2, "fail"),
                               (framed[:-1], "fail"), (framed + b"\r", "fail"),
                               (b"\x1b[200~\xff\x1b[201~", "fail")]:
            self.assertEqual(verdict(case, "kitty", {**self.evidence, "hex": data.hex()})[0], expected)

    def test_empty_partial_and_duplicate_reports_never_become_green(self):
        self.assertFalse(summarize({})["observed_checks_passed"])
        row = {**self.evidence, "case": "letter-a", "host": "stable", "path": "herdr", "mode": "legacy", "hex": "61", "phase": 1,
               "width": 120, "height": 30, "outer_geometry": [120, 30], "final_outer_geometry": [120, 30]}
        self.assertFalse(summarize({"observations": [row]})["observed_checks_passed"])
        with self.assertRaisesRegex(ValueError, "Duplicate"):
            summarize({"observations": [row, row]})
        later = {**row, "phase": 2, "width": 80, "height": 24, "outer_geometry": [80, 24], "final_outer_geometry": [80, 24]}
        host = {"channel": "stable", "runs": [{"nonce": "owned", "path": "herdr", "mode": "legacy", "pid": 123, "hwnd": 456,
                                                "elevated": False, "image_identity": "image-s", "installation_identity": "install-s"}]}
        row.update(nonce="owned", capture_id="first")
        later.update(nonce="owned", capture_id="second")
        self.assertEqual(summarize({"observations": [row, later], "hosts": [host], "controller_elevated": False})["counts"]["pass"], 2)
        stale = {**later, "capture_id": "first"}
        self.assertEqual(summarize({"observations": [row, stale], "hosts": [host], "controller_elevated": False})["counts"]["inconclusive"], 1)
        forged_hosts = [{"channel": name, "runs": [{}]} for name in ("stable", "preview")]
        partial = summarize({"observations": [row], "hosts": forged_hosts})
        self.assertFalse(partial["observed_checks_passed"])
        self.assertGreater(partial["coverage_missing"], 0)
        for status in ["unsupported", "not_run", "inconclusive"]:
            self.assertEqual(verdict(self.cases["letter-a"], "legacy", {**row, "status": status})[0], status)

    def test_malformed_native_records_are_inconclusive_not_exceptions(self):
        evidence = {**self.evidence, "scans": [30]}
        for records in [None, 7, "records", [None], [[1, 1, 1, 65, 30, 97, None]],
                        [[True, 1, 1, 65, 30, 97, 0]], [[1, 1, 1, 65, 30, 97, "0"]]]:
            self.assertEqual(verdict(self.cases["letter-a"], "native", {**evidence, "records": records})[0], "inconclusive")

    def test_known_host_gap_does_not_exempt_herdr_or_other_versions(self):
        row = {**self.evidence, "case": "shift-enter", "host": "stable", "path": "direct", "mode": "mok2", "hex": "0d", "phase": 1,
               "width": 120, "height": 30, "outer_geometry": [120, 30], "final_outer_geometry": [120, 30],
               "nonce": "owned", "capture_id": "fresh"}
        for path, version, raw, expected in [("direct", "1.24.11911.0", "0d", "unsupported"),
                                             ("herdr", "1.24.11911.0", "0d", "fail"),
                                             ("direct", "1.25.0.0", "0d", "fail"),
                                             ("direct", "1.24.11911.0", "", "fail")]:
            observation = {**row, "path": path, "hex": raw}
            run = {"nonce": "owned", "path": path, "mode": "mok2", "pid": 123, "hwnd": 456, "terminal_version": version,
                   "elevated": False, "image_identity": "image-s", "installation_identity": "install-s"}
            document = {"observations": [observation], "hosts": [{"channel": "stable", "runs": [run]}], "controller_elevated": False}
            result = summarize(document)["observations"][0]
            self.assertEqual(result["status"], expected)
            self.assertEqual(result["failure_scope"], "direct_host" if path == "direct" else "through_herdr_not_yet_attributed")
            document["controller_elevated"] = True
            self.assertEqual(summarize(document)["observations"][0]["status"], "inconclusive")
            document["controller_elevated"] = False
            run["elevated"] = True
            self.assertEqual(summarize(document)["observations"][0]["status"], "inconclusive")

    def test_duplicate_channel_identity_is_rejected_at_both_stages(self):
        hosts = [{"channel": name, "launcher_identity": name + "-exe", "installation_identity": name + "-dir",
                  "runs": [{"image_identity": name + "-image", "installation_identity": name + "-dir", "process_identity": name + "-pid/start"}]}
                 for name in ("stable", "preview")]
        self.assertEqual(channel_identity_errors(hosts), [])
        for field in ("launcher_identity", "installation_identity"):
            duplicate = copy.deepcopy(hosts)
            duplicate[1][field] = duplicate[0][field]
            self.assertTrue(channel_identity_errors(duplicate))
            self.assertTrue(summarize({"hosts": duplicate})["errors"])
        for field in ("image_identity", "installation_identity", "process_identity"):
            duplicate = copy.deepcopy(hosts)
            duplicate[1]["runs"][0][field] = duplicate[0]["runs"][0][field]
            self.assertTrue(channel_identity_errors(duplicate))


if __name__ == "__main__":
    unittest.main()
