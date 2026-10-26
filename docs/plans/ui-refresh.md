# UI refresh

Plan only. Do not treat this file as a change to the app.

> **For agentic workers:** Implement one task at a time. Do not add features that are not in this plan. Each task has a success check. Leave the version at `26.10.1` unless a later request says otherwise.

**Goal:** On a Windows release build, opening Miniature Catalog does not leave a command-prompt window on screen. The File menu is alphabetical, with Settings and Exit. The header is only the title. The Filters and Figures areas scroll separately, and the divider between them can be dragged.

**Architecture:** The window is one Rust process (`miniature-catalog-desktop`). It serves the UI on a localhost port inside that process and opens a Tauri webview. The menu, dialogs, theme, and layout are the existing plain TypeScript page (`frontend/src/main.ts` and `frontend/src/styles.css`). Reuse that page. Do not add a native menu, a drag library, or a second popup system.

**Tech stack:** Tauri 2 window, Axum in-process, Vite + TypeScript, `localStorage` for the theme (`mc.theme`).

## Global constraints

- Surgical changes only. No version bump, no new API routes, no catalog-data changes.
- Dialogs stay the ones already on the page: Close, Escape, backdrop click, and the existing focus trap. A new dialog must be a `<dialog>` so the current wiring covers it.
- Light and dark themes both have to work. The narrow layout is the existing `800px` breakpoint.
- The in-app File menu is not a Windows menu. Exit is a button in that menu.
- Agents cannot run Windows. Linux checks prove the code builds. Jason confirms the console on Windows.

## Assumptions

- "When the app runs" means the desktop window (`npm run tauri dev`, `npm run release`, or the installed `miniature-catalog-desktop` app), not `cargo run` from the repo root. `cargo run` is the headless API and is supposed to keep a console.
- The header "API token field" is the password box labeled by the placeholder "Admin or read token". It is not the admin-token text already shown inside Encounter generator.
- Alphabetical order uses the visible English label. Exit is not part of that sort. It stays last, after a separator, because Jason asked for it at the bottom.
- Settings holds the theme control and, when the page is not inside the desktop window, the token field. Encounter generator stays the place for the API switch, listen address, admin token, and API keys.
- A saved divider width is a preference, like the theme: write it only after the user changes it, and ignore a missing or broken value.
- This plan is based on `main` at `2a60720` (PR #5 is merged; `main` also contains the later PR #6 merge). The File menu on that commit is the one listed below.

## Karpathy

- Assumptions are the list above. Where a choice was open, the recommended default is in [Open questions](#open-questions).
- Minimum code: one crate attribute for the console, no child process, no log file, no drag library, the existing dialog loop, and `localStorage` the same way the theme already uses it.
- Each task ends with a check someone can run or look at. The last task is the pass across both themes and a narrow window.

## 1. Console window

### What actually opens it

`frontend/src-tauri/src/main.rs` is a normal Rust `fn main()`. There is no `#![windows_subsystem = "windows"]` anywhere in the repo. `frontend/src-tauri/build.rs` only calls `tauri_build::build()`. `tauri.conf.json` does not set a Windows subsystem. A Rust binary without that attribute is linked as a console program. Windows then opens a command prompt for it and keeps the prompt until the process exits. The prompt stays because the catalog process stays until the window closes.

The Tauri 2 template puts this at the top of the desktop `main.rs`, above every `use`:

```rust
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
```

This repo's desktop `main.rs` does not have that line. That is the fix Jason described.

### What it is not

- Not a second process. `launch()` builds a Tokio runtime in the desktop process, binds `127.0.0.1:0`, and `spawn_ui` runs Axum on a `tokio::spawn` task. The encounter-generator listener, when it is on, is another task in the same process (`ApiHost`). Nothing in the desktop shell calls `std::process::Command`.
- Not the headless binary. `src/main.rs` (`cargo run` from the repo root) prints the listen address and is meant to be a console program. Do not put `windows_subsystem` on that file.
- Not a build-script window. `build.rs` runs at compile time and does not stay open with the app.

`eprintln!` in the desktop `main.rs` (`catalog window:`, `catalog data:`, seed and API errors) is why text can appear in that prompt. Those lines can stay. In a dev build they still print. In a release build with the attribute above, Windows does not create a console, so those lines have nowhere to show. That is acceptable. Do not add a log file.

### Dev logs

`npm run tauri dev` compiles a debug binary (`debug_assertions` is set), so the attribute does not apply. Started from PowerShell, that process uses the PowerShell window it was started in. Jason reads logs there. A release build (`npm run release`, which is `tauri build`) has no console when the exe is double-clicked.

### Task

- [ ] Add the attribute above as the first line of `frontend/src-tauri/src/main.rs`. Do not change `src/main.rs`. Do not add the `windows` crate or `AttachConsole`.
- [ ] One sentence in the Windows section of `README.md`: a release build has no command prompt; `npm run tauri dev` still prints into the terminal that started it. `cargo run` from the repo root still keeps its console.

### Verify

- Linux: `cargo check --manifest-path frontend/src-tauri/Cargo.toml` (or the usual `npm run release` if a full build is already part of the pass). The attribute is ignored on Linux, so this only proves it compiles.
- Jason, on Windows: install or double-click `frontend\src-tauri\target\release\miniature-catalog-desktop.exe` after `npm run release`. No command prompt stays open. Close the window and confirm the process is gone.
- Jason, on Windows: from `frontend`, `npm run tauri dev`. The PowerShell that started it shows `catalog window:` and `catalog data:`. A second prompt does not appear.
- Jason, on Windows: `cargo run` from the repo root still opens a console and prints the API address. That is the headless API, not a regression.

## 2. File menu order, Settings, and the header

### Current File menu

On `main` today, top to bottom:

1. Add a miniature
2. Storage
3. Encounter generator
4. Edit filters

Help stays a separate menu with only About. Do not sort Help.

### Proposed File menu

Sort the action labels. Pin Exit last, under a separator. "Settings" falls between "Encounter generator" and "Storage" (`Se` before `St`). If Exit were sorted too, it would sit between "Encounter generator" and "Settings", and the separator would cut the list in half. Pinning it is what keeps "separator, then Exit" at the bottom.

1. Add a miniature
2. Edit filters
3. Encounter generator
4. Settings
5. Storage
6. separator (not a menu item)
7. Exit

Write that order in the HTML. Do not sort the buttons in JavaScript.

The separator is a `div` with `role="separator"`, not `role="menuitem"`. The existing arrow-key handler only moves `[role=menuitem]`, so it skips the separator. Give it a top border using the current menu colors. It is not tabbable.

### Settings popup

Add `<dialog id="settings-dialog">` next to the other dialogs, with a Close button. `File → Settings` calls the existing `showDialog`. Do not write a new focus trap.

Move, do not rewrite:

- `#theme-toggle` and `paintThemeToggle`. Same click behavior: flip light/dark and save `mc.theme` only after a click. First launch still follows the system and does not write storage.
- `#token-form` (the password input and Save token). Same `mc.token` key and the same submit handler.

On a desktop session, `boot()` already hides `#token-form` because `/api/v1/desktop-session` supplied the token. Keep that. Settings then shows the theme control only. In a plain browser, Settings shows the theme control and the token field.

Do not move the encounter-generator admin token, the API on/off switch, or the API keys into Settings.

After the move, the header is the `h1` only. Remove the subtitle paragraph and the empty `.header-actions` wrapper. Delete the CSS that only existed for that subtitle, the header token field, and the header theme button.

### Exit

- Desktop window (`'__TAURI_INTERNALS__' in window`): close the Tauri window with `@tauri-apps/api` (`getCurrentWindow().close()` from `@tauri-apps/api/window`). Add that package at Tauri 2, matching the CLI already in `frontend/package.json`. Import it only on the Exit click, so a browser session never calls it. Closing the last window ends the process, which also stops the in-process API. That is the same result as the window's close button.
- Plain browser: do not call `window.close()`. Browsers ignore that unless the page was opened by script. Put this in the existing `#message` line: `Close this browser tab to leave.`

### Tasks

- [ ] Remove the subtitle. Header shows "Miniature Catalog" and nothing under it.
- [ ] Add the Settings dialog and move the theme button and token form into it. Point File at it only after the item exists; the alphabetized markup in the next step can land in the same edit if that is easier, but check the dialog before rearranging the whole menu.
- [ ] Replace the File menu markup with the order in the table above, including the separator and Exit.
- [ ] Add `@tauri-apps/api` and the Exit click. Do not bump the app version. A new dependency does not change the `version` field `version:check` compares.

### Verify

- `npm --prefix frontend run typecheck` and `npm run build`.
- Browser, light and dark: File opens, items are in the order above, arrow keys skip the separator, Escape returns focus to File. Settings opens and closes with Close, Escape, and a backdrop click. Tab stays inside Settings. The theme button still flips both themes and survives a reload. The token field is in Settings, not in the header. The header has no subtitle.
- Browser: Exit does not close the tab. The message says to close the tab. Catalog data is unchanged.
- Desktop (`npm run tauri dev` on Linux is enough for this item): Exit closes the window. Jason, on Windows, confirms the same and that a listening encounter API stops when the window closes.
- Narrow width (`390px`): Settings fits, the menu still opens, the header does not overflow.

## 3. One scroll area each, header fixed

Today `body` grows with the page, so the whole window scrolls. `main` is a grid of `18rem` and `1fr`. At `800px` it becomes one column. The menu, header, and `#message` are normal flow, so they scroll away.

### Layout

- `html` and `body` fill the viewport (`100dvh`, with `100%` on `html` so percentage heights resolve). `overflow: hidden` on `body`. The page itself does not scroll.
- `#app` is a column flex: menu bar, header, `#message`, `main`. The first three do not shrink. `main` is `flex: 1` and `min-height: 0`.
- `#message` stays put so an error is not hidden under a long list. Cap it at about `4.5rem` and let that strip scroll if a message is huge.
- Each panel pins its `h2`. The name search stays in the Filters panel, above the scrolling list. `#filters` scrolls. `#results` scrolls. Both are `min-height: 0` and `overflow: auto`.
- `dialog.showModal()` draws in the top layer, so `overflow: hidden` on `body` does not clip the popups.
- At `max-width: 800px`, the panels stack. The page still does not scroll. Give Filters up to 40% of `main` and Figures the rest. Each still scrolls inside itself.

### Task

- [ ] Change `frontend/src/styles.css` as above. Do not change what the lists contain.

### Verify

- Browser, light and dark, wide window: with enough figures and filters to overflow, the menu and the title stay on screen while each panel scrolls on its own. The document scroll height equals the viewport (no page scrollbar).
- A long `#message` does not push the panels off the screen.
- At `390px`, both stacked panels scroll, and the page does not.
- Open Add, Settings, and About while a list is scrolled. The popup is fully on screen and still closes with Escape.

## 4. Draggable divider

Do this after the scroll layout, because the divider is the gap between the two panels.

### Behavior

- Between the panels, a vertical `role="separator"` with `aria-orientation="vertical"`, `tabindex="0"`, and `aria-label="Resize filters"`. Set `aria-valuemin`, `aria-valuemax`, and `aria-valuenow` in pixels.
- Default width stays `18rem` (what the grid uses today).
- Minimum `12rem` (192px). Maximum half of the window width (`window.innerWidth / 2`).
- Drag with the pointer (mouse or touch). Listen for move and up on `window` so the drag does not die if the pointer leaves the handle. `user-select: none` during the drag. The handle is wide enough to grab (about 6px) and uses the existing border color so both themes match.
- Keyboard, when the handle is focused: ArrowLeft and ArrowRight change the width by 16px. Home goes to the minimum. End goes to the maximum. `preventDefault` so the arrows do not scroll the list.
- Remember the width in `localStorage` under `mc.filtersWidth`, only after a drag ends or a key changes it. Same `try/catch` as `readTheme` / `applyTheme`. If the value is missing or not a number, use `18rem` and do not write.
- On every layout, clamp the displayed width to the minimum and to half the current window. Do not write that clamped value back. A later, wider window shows the saved width again.
- At `max-width: 800px`, hide the handle, take it out of tab order, and ignore drag and arrows. Keep the saved number. When the window is wide again, apply it.

No library. No new server state.

### Task

- [ ] Add the handle in `frontend/src/main.ts` and the grid column in `frontend/src/styles.css`. Keep the width logic in `main.ts` next to the other page behavior. Do not put it in `theme.ts` (that file is only the theme).

### Verify

- Browser, light and dark: drag the handle. Filters grow and stop at half the window. They do not shrink below 192px. Figures keep the rest and still scroll on their own.
- Reload. The width is the one the user left. Clear the `mc.filtersWidth` key. The width returns to `18rem` and the key stays absent until the next drag.
- Focus the handle. Arrow keys, Home, and End move it. Tab reaches it. The filters list does not scroll while an arrow is pressed.
- At `390px` the handle is gone, both panels stack and scroll, and widening the window brings the saved width back.
- Shrink a wide window until half the window is less than the saved width. The panel clamps. Widen it. The saved width returns without a new drag.

## Files

| File | Change |
| --- | --- |
| `frontend/src-tauri/src/main.rs` | The one `windows_subsystem` attribute. |
| `README.md` | One sentence on the release console versus `tauri dev` and `cargo run`. |
| `frontend/package.json` and `frontend/package-lock.json` | Add `@tauri-apps/api` at version 2. No app version bump. |
| `frontend/src/main.ts` | Menu order, Settings dialog, move theme and token, Exit, scroll structure if a wrapper is required, divider handle. |
| `frontend/src/styles.css` | Header cleanup, menu separator, fixed page, panel scroll, divider, narrow rules. |
| `frontend/src/theme.ts` | No change unless a call site must move with the button. Persistence stays `mc.theme`. |

Do not change `src/main.rs`, `openapi.yaml`, the database, or the version files.

## Suggested order

1. Console attribute and the README sentence. Independent, and Jason can check Windows as soon as a release build exists.
2. Remove the subtitle.
3. Settings dialog. Move the theme button and the token form.
4. Alphabetize File, add the separator and Exit.
5. Stop the page from scrolling. Make each panel scroll.
6. Divider, keyboard, and `mc.filtersWidth`.
7. Full pass: typecheck, build, both themes, `390px`, and the Windows console checks.

## Risks

- A release build that fails before the window opens will not show `eprintln!`. The recovery is `npm run tauri dev`, which still prints. Do not add a log file to cover this.
- `windows_subsystem` does nothing for `tauri build --debug` or `tauri dev`. If a console remains on a build Jason thought was release, confirm the exe is `target\release\miniature-catalog-desktop.exe`, not `target\debug`.
- `100dvh` is supported in current WebView2. If an old WebView2 ignores it, the `html { height: 100% }` chain is the fallback. Jason should say if the window still scrolls after the change.
- Hiding the token form on desktop is easy to break if the node is recreated when Settings opens. Leave the form in the dialog from the first render and only toggle `hidden`, which is what `boot()` does today.
- A static import of `@tauri-apps/api` would run in the browser build's graph. Call it from a dynamic `import()` inside the desktop check so a browser click never touches the IPC.
- Dragging can select text or scroll a panel. `preventDefault` on pointer down and `user-select: none` during the drag are part of the task, not a follow-up.

## Open questions

Each one has a default. Use the default unless Jason says otherwise. Do not block the work on these.

| Question | Default |
| --- | --- |
| Write release logs to a file under the app data folder? | No. Dev logs stay in the terminal that ran `npm run tauri dev`. |
| Show the token field in Settings inside the desktop window? | No. Keep today's rule: a desktop session hides it. The theme control is always there. The encounter dialog still shows the admin token. |
| What should Exit do in a plain browser? | Leave the tab open and set `#message` to `Close this browser tab to leave.` |
| Where does Exit sit relative to alphabetical order? | Last, after a separator. The other items, including Settings, are sorted by label. |
| Sort the menu in code or write the order in HTML? | HTML, in the order in section 2. |
| Pixels or a fraction for the saved width? | Pixels in `mc.filtersWidth`. Display is clamped to `12rem` through half of `window.innerWidth`. The stored number is not rewritten when the clamp changes. |
| What happens at the narrow breakpoint? | Below `800px`, hide the handle and stack the panels. Each scrolls. Keep the saved width for the next wide layout. |
| Do the "Filters" and "Figures" headings scroll away? | No. Pin each heading. Scroll the list under it. The name search stays pinned with the Filters heading. |
| Put `windows_subsystem` on the headless `cargo run` binary too? | No. That program is a console API. |

## Done when

- A Windows release exe, double-clicked, shows the catalog window and no command prompt. `npm run tauri dev` still prints into its terminal. `cargo run` still has a console.
- File reads, top to bottom: Add a miniature, Edit filters, Encounter generator, Settings, Storage, a separator, Exit.
- The header is the title only. Theme and the browser token field are in Settings. Settings follows the existing popup rules in both themes and at `390px`.
- Exit closes the desktop window and the process. In a browser it only shows the close-the-tab message.
- The page does not scroll. Filters and Figures scroll separately. The menu and title stay put.
- The divider drags and responds to arrows, stops at `12rem` and at half the window, and comes back after a restart. It is absent when the window is narrow.
- `npm --prefix frontend run typecheck` and `npm run build` pass. No version file changed.
