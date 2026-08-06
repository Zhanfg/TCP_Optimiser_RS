# WebUI restart audit

Date: 2026-08-06

## Why the previous pass was incomplete

The first restart pass focused on kernel-state rollback and module lifecycle safety. The existing WebUI still passed its old CI because that validation only checked JavaScript syntax, translation-key consistency and file existence. It did not verify routing, responsive layout, modal behavior, log usability or real browser rendering.

## Confirmed defects addressed

### Application shell and navigation

- Startup previously replaced every location with `#home`, so direct links and restored pages were lost.
- Browser back/forward navigation did not restore the active page or its reading position.
- The desktop breakpoint reused the mobile floating bottom navigation because later CSS declarations overrode the intended desktop layout.
- Mobile navigation did not consistently account for gesture insets and very narrow screens.
- The top title was replaced by the current navigation label instead of preserving product identity and page context.
- Polling continued while the WebView was hidden.

### Logs

- File-read failures were represented as an empty log list.
- Every refresh rebuilt the entire list and forced the reader to the bottom.
- Only `service.log` was visible; debug and restoration evidence were hidden.
- The page lacked filtering, copying, manual refresh and controllable tail-following.
- Clearing logs had no confirmation.
- Unbounded file reads could stall the WebView when logs grew.

### Settings and advanced controls

- Mobile users could open many large settings groups simultaneously, producing a long, difficult-to-navigate page.
- The primary apply actions could scroll behind the bottom navigation.
- Immediate policy application had no confirmation step.
- The 39 advanced sysctl controls had no search or filtering.
- Selectable chips did not consistently expose `aria-pressed` or `aria-disabled` state.
- The debug FAB had contradictory CSS display declarations and could be visible in the wrong state or remain hidden after enabling debug mode.

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

- the post-legacy production stylesheet;
- the desktop navigation rail;
- safe-area-aware mobile navigation;
- deep-link restoration and hidden-page polling suspension;
- bounded multi-source logs and visible read errors;
- searchable advanced settings and force-apply confirmation;
- dialog focus management and debug FAB synchronization;
- read-only baseline health reporting.

## Browser validation matrix

The candidate package must be rendered with the explicit browser-preview bridge at these minimum viewports:

- 320 × 720: minimum supported narrow Android WebView;
- 393 × 852: representative modern phone;
- 768 × 1024: tablet / large embedded WebView;
- 1440 × 960: desktop browser layout.

For each viewport, inspect Home, Statistics, Settings, Logs and Advanced pages for horizontal overflow, clipped controls, hidden navigation labels, dialog focus, sticky action placement and readable chart/log content.

## Remaining device-only validation

Browser preview cannot validate KernelSU/Magisk bridge behavior, Android back dispatch, system dynamic-color resources, haptic feedback, real `tc`/sysctl output or vendor-specific WebView differences. Those checks remain part of `docs/RESTART-VALIDATION.md` and must be completed before merging or publishing a stable release.
