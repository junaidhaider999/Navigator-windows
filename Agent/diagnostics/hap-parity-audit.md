# Hunt-and-Peck parity audit (Navigator)

## Root causes (why Navigator showed fewer / worse hints)

1. **Invoke-only discovery**  
   Enumeration required `InvokePattern` (FindAll condition + per-element filter). Hunt-and-Peck uses a much broader notion of “actionable” UI (MSAA default actions, toggles, list items, hyperlinks, focusable controls, etc.). Anything without `InvokePattern` never appeared.

2. **Aggressive provider-side filter**  
   The descendant `FindAll` condition was effectively “invoke-capable only,” so the UIA tree walk never returned other interaction candidates.

3. **Dispatch = Invoke only**  
   Even if a control were listed, activation always used `IUIAutomationInvokePattern::Invoke`. Toggle, selection, expand/collapse, legacy default actions, and plain hit-targets were unsupported.

4. **Stray / odd pill placement (secondary)**  
   Pills were corner-anchored first; dense UIA (e.g. list rows) plus de-collision could leave labels offset from the perceived click target. Some “dead space” pills likely corresponded to real nodes whose bounding rects were outside the visible client area or mis-partitioned across monitors.

## Fixes implemented (code)

- **Broad FindAll condition**: OR of “pattern available” flags (Invoke, Toggle, SelectionItem, ExpandCollapse, LegacyIAccessible, Value) OR (`IsKeyboardFocusable` AND control type in a short list of interactive types).
- **Cache**: Enumeration and find-all caches now include the above patterns plus `ControlType` and `IsKeyboardFocusable` for classification.
- **Collection**: Classify each candidate to `ElementKind`; optional `[uia-debug]` stderr lines for skipped nodes; drop hints whose **center** lies outside the **session root** `GetWindowRect` (reduces off-window noise).
- **Dispatch**: Pattern-specific actions + `LegacyIAccessible::DoDefaultAction` where applicable + **left-click center fallback** for generic/editable targets.
- **Planner cap**: Default `max_elements` raised to **2048**.
- **Placement**: Pill layout tries **element center** before corner rings (`scene.rs`).

## Debug mode

Run the app with **`--debug-uia`** to print skip reasons while enumerating (`[uia-debug]` on stderr).

Run with **`--debug-overlay`** (in addition or alone) to draw **translucent orange rectangles** for nodes that matched the UIA `FindAll` filter but were dropped in Rust (same skip reasons as stderr logging, when bounds are known). If the planner yields no pills but rejects have geometry, the overlay still appears; press **Escape** to dismiss.

## Metrics / next steps

- For each target app, compare hint counts and obvious misses vs HAP after these changes.
- If coverage is still short in Chromium hosts, consider a second-pass strategy (e.g. RawView / role-based MSAA) scoped behind a flag.
- Optional: structured JSON logs and a small bench harness comparing `took_ms` before/after.
