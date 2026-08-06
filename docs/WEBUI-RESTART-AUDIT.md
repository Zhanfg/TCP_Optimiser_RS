# WebUI restart audit

Date: 2026-08-06

## Why the previous pass was incomplete

The first restart pass focused on kernel-state rollback and module lifecycle safety. The existing WebUI still passed its old CI because that validation only checked JavaScript syntax, translation-key consistency and file existence. It did not verify routing, responsive layout, modal behavior, log usability, configuration consistency or real browser rendering.

## Confirmed defects addressed

### Application shell and navigation

- Startup previously replaced every location with `#home`, so direct links and restored pages were lost.
- Browser back/forward navigation did not restore the active page or its reading position.
- The desktop breakpoint reused the mobile floating bottom navigation because later CSS declarations overrode the intended desktop layout.
- The desktop Home page declared two grid columns without changing the inherited flex layout, leaving the intended status column unused.
- Mobile navigation did not consistently account for gesture insets and very narrow screens.
- The top title was replaced by the current navigation label instead of preserving product identity and page context.
- Polling continued while the WebView was hidden.

### Logs

- File-read failures were represented as an empty log list.
- Every refresh rebuilt the entire list and forced the reader to the bottom.
- Only `service.log` was visible; debug and restoration evidence were hidden.
- The page lacked filtering, copying, manual refresh and controllable tail-following.
- Filtering could leave source headings with no matching log entry.
- Clipboard permission failure had no compatibility fallback.
- Clearing logs had no confirmation and did not guard an unavailable module directory.
- Unbounded file reads could stall the WebView when logs grew.
- Switching language updated toolbar labels but left generated source and file-status messages in the previous language.

### Settings, presets and advanced controls

- Mobile users could open many large settings groups simultaneously, producing a long, difficult-to-navigate page.
- The ordinary Settings apply row could scroll behind the bottom navigation.
- A bottom-sticky Advanced apply button was pulled into view before its normal position and covered the first visible sysctl controls. The Advanced action now remains in document flow; mobile groups use single-open disclosures to keep it reachable without hiding input data.
- Immediate policy application had no confirmation step.
- The 39 advanced sysctl controls had no search or filtering.
- Selectable chips did not consistently expose `aria-pressed` or `aria-disabled` state.
- The initial settings enhancement watched attributes that it also modified, allowing a self-triggering MutationObserver loop. Synchronization now uses bounded page, click and change events.
- The debug FAB had contradictory CSS display declarations and could be visible in the wrong state or remain hidden after enabling debug mode.
- Preset application performed algorithm markers, module files, qdisc and live sysctl writes as separate steps. Failure in a later step could leave an unlabelled partial preset.
- Successful preset application updated runtime files but did not synchronize the visible algorithm, qdisc and toggle controls.

Preset clicks now pass through a dedicated transaction with:

- a process lock and stale-lock recovery;
- backups of managed module files and algorithm markers;
- snapshots of live qdisc, TCP Fast Open and ECN values;
- atomic temporary-file replacement for persisted settings;
- readback verification for live sysctl writes;
- signal/error rollback and cleanup;
- UI state synchronization only after a committed transaction.

A local shell simulation verified both the success path and an injected late failure. The injected failure restored algorithm markers, qdisc, TCP Fast Open, ECN, pacing files and the previous `force_apply` state.

### Dialogs and accessibility

- Dialogs lacked centralized focus entry, focus trapping, Escape dismissal and focus restoration.
- Background scrolling remained active behind dialogs.
- Navigation did not implement roving keyboard focus.
- The application had no skip link, live refresh announcement or explicit global refresh action.

### Rollback visibility

- Transactional baseline health was only available by manually inspecting module files.
- The WebUI now uses the read-only `baseline-status` command. Opening the UI never invokes `capture-baseline` and therefore cannot create rollback evidence after tuning begins.

## New regression gates

`scripts/validate-webui.mjs` now rejects changes that remove or bypass:

- the post-legacy production and final layout stylesheets;
- the desktop navigation rail and actual desktop Home grid;
- safe-area-aware mobile navigation;
- deep-link restoration and hidden-page polling suspension;
- bounded multi-source logs, visible read errors and compatibility copy behavior;
- searchable advanced settings and force-apply confirmation;
- dialog focus management and debug FAB synchronization;
- read-only baseline health reporting;
- importable JavaScript modules and packaged production files.

Temporary `.fixed`, `.tmp` and cleanup-marker files are rejected from the packaged WebUI.

## Browser validation matrix

The candidate package is rendered from the actual assembled flashable ZIP at these minimum viewports:

- 320 × 720: minimum supported narrow Android WebView;
- 393 × 852: representative modern phone;
- 768 × 1024: tablet / large embedded WebView;
- 1440 × 960: desktop browser layout.

For each viewport, Home, Statistics, Settings, Logs and Advanced are checked for horizontal overflow, clipped controls, hidden navigation labels, dialog focus, action placement and readable chart/log content. Advanced mode is also initialized with its persisted opt-in so all 39 controls are rendered rather than testing only the disabled placeholder page.

The preset transaction is exercised through the rendered Settings page. A committed Gaming preset updates the selected Wi-Fi algorithm, cellular algorithm, qdisc, toggles, active preset marker and router state without JavaScript errors.

## Remaining device-only validation

Browser preview cannot validate KernelSU/Magisk/APatch bridge behavior, Android back dispatch, system dynamic-color resources, haptic feedback, real `tc`/sysctl output or vendor-specific WebView differences. Those checks remain part of `docs/RESTART-VALIDATION.md` and must be completed before merging or publishing a stable release.
