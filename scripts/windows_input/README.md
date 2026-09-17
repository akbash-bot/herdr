# Local Windows input gauntlet (experimental)

This is **test infrastructure awaiting native Windows qualification**, not a claim
that all Windows input works. It sends real scan-code gestures through a new
Windows Terminal window, both directly to an observer and through an attached
Herdr client. It does not substitute `pane send-keys` for host input.

## Safety and prerequisites

Use an **unlocked, isolated interactive desktop** where you will not do other
work during automated input. The runner changes foreground focus and window
size. `SendInput` has an unavoidable check-to-use focus race; a foreground check
is not an OS security boundary. Do not run this against an administrator window
or in a shared working desktop. There is no unattended CI job or runner permission
change in this implementation.

Required: Windows, installed PowerShell **7** (`pwsh`), Python 3, and an existing
Herdr Windows executable with its adjacent bundled ConPTY directory. Stable and
Preview Windows Terminal are discovered independently through their installed
packages. Explicit paths are available when discovery does not work. No software
is installed or updated. The actual Terminal process path/version is recorded,
not inferred from the requested channel or bundled OpenConsole version.

**F12** aborts before the next automatic gesture. Moving focus away also aborts
further automatic input. Each injected chord contains its own releases; there
are no intentionally held modifiers between calls. Do not hold physical modifiers
or mouse buttons while starting a run. Normal controller cancellation runs cleanup;
bootstrap/probe leases also expire if the controller disappears. Lease expiry is
not a replacement for a secure isolated desktop.

Clipboard tests require an **empty clipboard**. Clear it yourself only after
saving anything you need. The runner will not replace existing text, images,
rich formats, or files. It writes its synthetic text while holding the clipboard
lock and clears it afterward only if its sequence number is unchanged. A later
user clipboard update is left alone. No original clipboard contents are logged.

## Run

From the repository in PowerShell 7:

```powershell
just test-windows-input -ExePath 'C:\test-app\herdr.exe' -AllowInputInjection
```

Or invoke the script directly:

```powershell
pwsh -NoProfile -File scripts/test_windows_input.ps1 `
  -ExePath 'C:\test-app\herdr.exe' -AllowInputInjection `
  -StablePath 'C:\TerminalStable\WindowsTerminal.exe' `
  -PreviewPath 'C:\TerminalPreview\WindowsTerminal.exe'
```

The default tests current Herdr's **default** input policy. Diagnostic runs may
use `-Profile win32` or `-Profile vt`; they do not replace the default run.
`-Modes native,legacy,mok2,kitty`, `-Widths`, and `-Heights` select observer modes
and geometry. In a direct PowerShell invocation, supply arrays normally:

```powershell
.\scripts\test_windows_input.ps1 -ExePath 'C:\test-app\herdr.exe' `
  -AllowInputInjection -Modes mok2,kitty -Widths 119,120,121,160 -Heights 24
```

`-Manual` enables guided composition cases. The operator must activate the
indicated layout/IME, perform the gesture in the test window, then return to the
controller and press Enter. Finish each prompt within the 90-second lease.
Record the layout you actually selected in your qualification notes; the report
also includes the observed foreground thread layout handle. Do not paste text
for a composition case. The runner does not install/change global layouts.

You can inspect the full catalogue without Windows or desktop interaction:

```powershell
pwsh -NoProfile -File scripts/test_windows_input.ps1 -ExePath unused -MatrixOnly
```

Every run needs a **new** output directory. By default it is
`.local/windows-input/<unique-id>/`. Do not reuse an old report directory.

## What the implementation measures

- Native console records, including down/up, repeat, virtual/scan codes, Unicode,
  modifier state, and raw payloads for other native record types.
- Legacy VT, modifyOtherKeys level 2, and Kitty disambiguation observer modes.
  These are **pane consumer modes**, not names for Herdr's outer reader.
- Physical-style Enter/Shift+Enter and other modifier chords, navigation and word
  movement chords, control keys, and editing keys. The oracle checks bytes or
  records; it does not claim that a particular editor implements word selection
  correctly merely because Ctrl+Shift+Right reached it.
- Actual Windows clipboard paste via Ctrl+V, including LF/CRLF/CR, whitespace,
  Unicode/combining characters, escape-looking text, and a 200-line burst.
  A host binding or multiline-paste confirmation dialog can intercept the gesture;
  the runner does not dismiss unexpected dialogs or rebind Terminal shortcuts.
- A full case pass at an observed 120×30 host size; keyboard/paste sentinels at
  **80, 119, 120, 121, 132, 160, 240 columns**, at 24 and 50 rows; then return to
  80 columns. This exercises narrow→wide→narrow resizing of the actual outer
  window. Both outer and pane dimensions are captured. Herdr chrome means pane
  width is not the same as outer width. An unreachable size is not a pass.
- Guided accent/AltGr/IME commits, with exact expected committed text.

The catalogue also lists explicit **qualification gaps**: mouse click/drag/wheel
and right-edge coordinate mapping; visual reflow/wrapping; native held-key repeat;
lock/keypad combinations; dead-key cancellation; IME cancellation; capture/config
reload and attach cycles; injected setup/recovery faults; image/file clipboard
integrations. These are recorded `not_run`, not fabricated successes. They need
separate fixtures/oracles before becoming automated assertions. The catalogue is
broad; this draft is **not fully automated coverage of every row**.

## Evidence and verdicts

The retained directory contains:

- `matrix.json`: all cases, profiles, and declared byte/record expectations;
- `observations.json`: raw captures, per-case identity, measured dimensions,
  readiness/focus flags, errors, and cleanup results;
- `report.json`: interpreted verdicts and counts;
- per-window plans, nonce-bound observer acknowledgements, initial/final console
  modes, and bootstrap/probe error records.

Direct-host and through-Herdr observations are labelled separately. A direct-host
failure is **not automatically a Herdr bug**. For example, a terminal may intercept
Alt+Enter or use a different control-key encoding. Review captures before changing
an expectation; never bless lost Shift information just to make a report green.

Comparisons consume the **complete captured sequence**, including a quiet interval
to catch trailing duplicates/releases. Paste must have one intact bracketed
wrapper and the complete payload. CR/LF variants are compared as logical newlines;
this is not proof of byte-exact line-ending policy. Native keys require their
non-modifier down/up pair, modifier state, and repeat count.

Exit codes:

- `0`: all observed assertions passed and no recorded coverage gaps/errors;
- `1`: an assertion, harness, or cleanup failed;
- `2`: coverage is incomplete (including deliberately unimplemented catalogue
  rows, missing hosts, or unavailable geometry).

An empty or missing report never means success. Current full-catalogue runs will
normally return **2 even if automated checks pass**, until the listed qualification
gaps acquire evidence. None of these statuses certifies an entire Windows host.

## Cleanup and native handoff

Every window has a fresh nonce, named session, isolated config root, known
PowerShell pane shell, and observer acknowledgement. Inherited Herdr socket,
backend, session, and SSH variables are cleared in the actual launched process,
not just in the controller. Setup APIs only create/focus the observer pane.

Cleanup targets the named session, retained child-process handles, and the exact
nonce-bearing Terminal window. There is no process-name-wide kill or sweep of
newly appeared OpenConsole processes. Artifacts are retained. Forced cleanup and
console-mode mismatches are reported, not hidden behind a passing key test.

Before calling this ready, the Windows execution partner should qualify Stable
and Preview, real focus-loss/F12 interruption, partial startup, clipboard changes,
cleanup, and measured width boundaries. Use the old and new Herdr binaries in
**separate runs** to establish that a known Shift+Enter/paste regression is caught.
Do not overwrite installations or reuse an earlier server. Native results are
currently pending; Linux unit tests only validate the catalogue and verdict logic.

The existing `windows_conpty_enhanced_input_probe.ps1` remains the downstream API
control. Existing Rust keyboard/Windows-translator tests remain the deterministic
parser controls. This runner complements them rather than cross-producting their
fixtures or pretending they establish host behavior.
