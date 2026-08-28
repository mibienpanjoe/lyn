# Lyn — Visual Identity and Interface System

Version: v1.1, 2026-08-28

Derived from: [`01_requirements_prd.md`](01_requirements_prd.md), [`02_requirements_srs.md`](02_requirements_srs.md), and [`05_architecture.md`](05_architecture.md)

Design status: **Proposed v1 system.** The overview defines the desired character—“Raycast × Linear × a tiny developer notebook”—but does not prescribe tokens. The concrete values below are implementation decisions derived from speed, calmness, precision, local ownership, and compact desktop use. They require visual and accessibility validation in the running Tauri WebView.

## 1. Brand Essence

### Product expression

Lyn is a quiet extension of working memory. It appears when invited, captures without ceremony, and leaves the user's attention where it was.

The name's origin is not defined in the source overview. The product MUST NOT invent an etymology or mascot story until the owner establishes one.

### Personality

- **Immediate:** ready before the user has to wait or orient themselves.
- **Calm:** low visual noise, no gamification, no urgency theater.
- **Precise:** crisp hierarchy, stable alignment, predictable keyboard behavior.
- **Personal:** closer to a small notebook than an enterprise content system.
- **Technical without being cold:** contextual metadata is useful but visually subordinate to the thought.

### Design principles

1. **The thought is the primary object.** Text, image, or audio comes before controls and metadata.
2. **Capture has no ceremony.** No page title, form grid, rich-text toolbar, tag wall, or confirmation step on the happy path.
3. **Context whispers.** Project and branch remain visible but never compete with the capture body.
4. **Density serves flow.** Use compact, deliberate spacing; do not inflate the utility into a dashboard.
5. **Motion spends attention carefully.** High-frequency capture interactions are immediate; motion exists only where it clarifies state.
6. **Local feels trustworthy.** Avoid cloud-status decoration, AI sparkle motifs, and language that implies remote processing.
7. **Accessibility is structural.** Keyboard order, focus, contrast, labels, and reduced motion are part of every component definition.

### Visual anti-patterns

Lyn MUST NOT use:

- purple/indigo “AI product” gradients;
- frosted-glass layers that reduce text contrast;
- oversized rounded cards for every content group;
- decorative dashboard statistics;
- emoji as production control icons;
- excessive shadows, floating panels, or animated background effects;
- a mode selector before ordinary text entry;
- animated typing, pulsing AI marks, celebratory save effects, or modal success confirmations.

## 2. Color System

### Color strategy

The palette combines warm paper-like neutrals with a restrained jade signal color. Jade marks focus, selection, and primary intent; it is not painted across large decorative areas. Warm amber is reserved for caution and recording context, not for the primary brand.

Raw palette values are defined once as CSS custom properties. Components consume semantic tokens only.

### Light theme

| Semantic token | Name | Value | Use |
|---|---|---|---|
| `--color-canvas` | Paper | `#F6F7F3` | Library window background. |
| `--color-surface` | Sheet | `#FFFFFF` | Popup and primary surfaces. |
| `--color-surface-subtle` | Quiet paper | `#EEF1EB` | Hover rows, grouped controls, secondary areas. |
| `--color-surface-raised` | Clear sheet | `#FFFFFF` | Menus and elevated transient surfaces. |
| `--color-text` | Ink | `#171A17` | Primary text. |
| `--color-text-muted` | Graphite | `#5F675F` | Metadata and secondary copy. |
| `--color-text-faint` | Pencil | `#6B746A` | Placeholder text; never critical information. |
| `--color-divider` | Rule | `#DCE1D9` | Structural separators. |
| `--color-input-border` | Input rule | `#C8CEC5` | Input outline at rest. |
| `--color-accent` | Lyn Jade | `#286B59` | Primary actions, active focus, selected state. |
| `--color-accent-hover` | Deep Jade | `#205747` | Primary hover. |
| `--color-accent-soft` | Jade Wash | `#E3F0EA` | Selected row or context-chip background. |
| `--color-on-accent` | White | `#FFFFFF` | Text/icons on jade. |

### Dark theme

| Semantic token | Name | Value | Use |
|---|---|---|---|
| `--color-canvas` | Night Paper | `#101310` | Library window background. |
| `--color-surface` | Charcoal Sheet | `#171B17` | Popup and primary surfaces. |
| `--color-surface-subtle` | Moss Graphite | `#202620` | Hover rows and grouped controls. |
| `--color-surface-raised` | Raised Graphite | `#242A24` | Menus and elevated transient surfaces. |
| `--color-text` | Chalk | `#F1F4EF` | Primary text. |
| `--color-text-muted` | Sage Gray | `#A7B0A5` | Metadata and secondary copy. |
| `--color-text-faint` | Quiet Sage | `#8E988C` | Placeholder text; never critical information. |
| `--color-divider` | Night Rule | `#303830` | Structural separators. |
| `--color-input-border` | Night Input | `#3A443A` | Input outline at rest. |
| `--color-accent` | Lit Jade | `#79C4A9` | Primary actions, focus, selected state. |
| `--color-accent-hover` | Bright Jade | `#8DD2B8` | Primary hover. |
| `--color-accent-soft` | Jade Shade | `#18372D` | Selected row or context-chip background. |
| `--color-on-accent` | Deep Ink | `#102018` | Text/icons on lit jade. |

### Semantic status colors

| Token | Light | Dark | Meaning |
|---|---|---|---|
| `--color-success` | `#237A45` | `#6BCB8B` | Durable save, installed local model. |
| `--color-success-soft` | `#E4F3E8` | `#163522` | Subtle success surface. |
| `--color-warning` | `#9A5B13` | `#E5A44F` | Permission or recoverable caution. |
| `--color-warning-soft` | `#F8ECD9` | `#3A2915` | Warning surface. |
| `--color-error` | `#B4232C` | `#FF858B` | Failed save, destructive action, unavailable media. |
| `--color-error-soft` | `#FBE7E8` | `#3C191C` | Error surface. |
| `--color-info` | `#34689A` | `#81B6E8` | Neutral system information. |
| `--color-info-soft` | `#E5EEF7` | `#172D42` | Information surface. |
| `--color-recording` | `#B53A32` | `#FF8077` | Active microphone recording only. |

Status MUST pair color with text, icon, shape, or live-region output. Recording uses both a filled dot and “Recording” label.

### CSS foundation

```css
:root {
  color-scheme: light dark;

  --color-canvas: #f6f7f3;
  --color-surface: #ffffff;
  --color-surface-subtle: #eef1eb;
  --color-surface-raised: #ffffff;
  --color-text: #171a17;
  --color-text-muted: #5f675f;
  --color-text-faint: #6b746a;
  --color-divider: #dce1d9;
  --color-input-border: #c8cec5;
  --color-accent: #286b59;
  --color-accent-hover: #205747;
  --color-accent-soft: #e3f0ea;
  --color-on-accent: #ffffff;

  --color-success: #237a45;
  --color-warning: #9a5b13;
  --color-error: #b4232c;
  --color-info: #34689a;
  --color-recording: #b53a32;

  --shadow-surface:
    0 0 0 1px oklch(0 0 0 / 0.06),
    0 1px 2px -1px oklch(0 0 0 / 0.06),
    0 8px 24px -8px oklch(0 0 0 / 0.14);
  --shadow-surface-hover:
    0 0 0 1px oklch(0 0 0 / 0.08),
    0 1px 2px -1px oklch(0 0 0 / 0.08),
    0 10px 28px -8px oklch(0 0 0 / 0.16);
}

@media (prefers-color-scheme: dark) {
  :root:not([data-theme="light"]) {
    --color-canvas: #101310;
    --color-surface: #171b17;
    --color-surface-subtle: #202620;
    --color-surface-raised: #242a24;
    --color-text: #f1f4ef;
    --color-text-muted: #a7b0a5;
    --color-text-faint: #8e988c;
    --color-divider: #303830;
    --color-input-border: #3a443a;
    --color-accent: #79c4a9;
    --color-accent-hover: #8dd2b8;
    --color-accent-soft: #18372d;
    --color-on-accent: #102018;

    --color-success: #6bcb8b;
    --color-warning: #e5a44f;
    --color-error: #ff858b;
    --color-info: #81b6e8;
    --color-recording: #ff8077;

    --shadow-surface: 0 0 0 1px oklch(1 0 0 / 0.08);
    --shadow-surface-hover: 0 0 0 1px oklch(1 0 0 / 0.13);
  }
}
```

An explicit `[data-theme="dark"]` token block MUST mirror the dark media-query values so the `system`, `light`, and `dark` settings all work deterministically.

### Contrast rules

- Normal text MUST meet WCAG 2.1 AA contrast of at least 4.5:1.
- Large text and meaningful non-text graphics MUST meet at least 3:1.
- Disabled text is exempt only when the control is genuinely unavailable and its state remains understandable.
- `--color-text-faint` is restricted to placeholders and supplementary copy after contrast validation; labels and timestamps use `--color-text-muted`.
- Focus rings MUST remain at least 3:1 against adjacent colors in both themes.

## 3. Typography

### Font strategy

Lyn uses the native system UI stack to minimize startup cost, avoid network font loading, and feel at home on each desktop platform.

```css
--font-ui: ui-sans-serif, -apple-system, BlinkMacSystemFont,
  "Segoe UI Variable", "Segoe UI", sans-serif;

--font-mono: ui-monospace, "SFMono-Regular", Menlo, Monaco,
  Consolas, "Liberation Mono", monospace;
```

No external font request is permitted. A future bundled brand font requires an ADR covering license, package weight, fallback metrics, and startup impact.

### Weight and rendering

- `400`: capture body, note text, descriptive copy.
- `500`: metadata, labels, controls, active navigation.
- `600`: page and section headings only.
- Avoid `700+` in the compact application UI.
- Use tabular numerals for timestamps, durations, byte counts, and progress.
- Do not reduce opacity on text; use a semantic text color so contrast is testable.

### Type scale

| Token | Size | Line height | Weight | Use |
|---|---:|---:|---:|---|
| `--text-xs` | 11px | 16px | 500 | Compact badge text; avoid for essential metadata. |
| `--text-sm` | 12px | 17px | 400/500 | Context, branch, timestamps, helper text. |
| `--text-body` | 13px | 19px | 400 | Library rows and standard controls. |
| `--text-input` | 15px | 22px | 400 | Quick-capture text and captions. |
| `--text-section` | 16px | 22px | 600 | Library group headings and empty-state titles. |
| `--text-page` | 20px | 27px | 600 | Library page title. |
| `--text-display` | 24px | 31px | 600 | Rare onboarding/empty-state headline only. |

Text notes in the Library preserve line breaks. Long paths and branch names use middle truncation with the full value available in an accessible tooltip; capture text wraps naturally and is never ellipsized in detail view.

## 4. Spacing and Layout

### Spacing scale

The base unit is 4px.

| Token | Value | Typical use |
|---|---:|---|
| `--space-1` | 4px | Icon-to-label micro-gap. |
| `--space-2` | 8px | Inline controls, row subgroups. |
| `--space-3` | 12px | Compact row padding. |
| `--space-4` | 16px | Standard panel padding. |
| `--space-5` | 20px | Popup horizontal inset. |
| `--space-6` | 24px | Library section separation. |
| `--space-8` | 32px | Major empty-state or page separation. |

Arbitrary spacing values are not allowed unless required for optical correction and documented in the component.

### Radius scale and concentric rule

| Token | Value | Use |
|---|---:|---|
| `--radius-control` | 6px | Buttons, inputs, chips. |
| `--radius-media` | 8px | Screenshot and waveform previews. |
| `--radius-panel` | 12px | Menus, Library detail surfaces. |
| `--radius-popup` | 14px | Quick-capture window surface. |
| `--radius-pill` | 999px | Recording dot container or true status pills only. |

Nested corners MUST be concentric when their inset is visible:

```text
outer radius = inner radius + inset
```

Example: a 14px popup with a text surface inset by 6px uses an 8px inner radius. Equal outer and inner radii are prohibited because they make the inner surface appear pinched.

### Window geometry

#### Quick Capture

- Default content width: 560px.
- Supported responsive range: 360–680px.
- Initial text-only minimum height: 168px.
- Expanded screenshot/voice height: content-driven up to 440px, then internal scrolling.
- Outer padding: 6px shell inset plus 14–20px content spacing.
- The popup is one visual surface, not a stack of cards.

#### Library

- Default window: 1040 × 720px.
- Minimum usable window: 640 × 480px.
- Navigation rail: 208px at ≥900px.
- Capture stream readable column: 560–720px; detail panel uses remaining width when available.
- At <900px, detail replaces the stream with a Back action instead of squeezing both.
- At <720px, the navigation rail becomes a compact drawer or top-level switcher.
- At 320–479px test width, the interface must remain operable as a single column even though no mobile release is implied.

### Responsive verification widths

Test Library and settings at 320px, 768px, 1024px, and 1440px content widths. Test quick capture at 360px, 440px, 560px, and 680px. No horizontal scrolling is permitted in primary flows.

## 5. Elevation and Surfaces

- Use structural `1px` borders for inputs, list separators, and selected regions.
- Use `--shadow-surface` for the outer popup, menus, and truly elevated transient elements.
- Do not place every Library row in a card. Rows share one stream with dividers and hover/selected backgrounds.
- In dark mode, elevation uses a subtle white ring rather than black ambient shadows.
- Hover elevation transitions only `box-shadow` over 150ms; `transition: all` is prohibited.

Screenshot previews MUST use an inside outline independent of the theme palette:

```css
.media-preview {
  outline: 1px solid oklch(0 0 0 / 0.1);
  outline-offset: -1px;
}

@media (prefers-color-scheme: dark) {
  .media-preview {
    outline-color: oklch(1 0 0 / 0.1);
  }
}
```

The outline MUST be pure black in light mode and pure white in dark mode, not a tinted neutral or accent color.

## 6. Icon System

Use one outline SVG icon family with a native 16px or 20px grid. Lucide for Svelte is the proposed implementation because it is consistent and tree-shakable; adopting it is a dependency decision, not a claim that it is installed.

Rules:

- Icons use `currentColor`; do not hardcode state-specific fills.
- Default inline icon size is 16px; primary icon-only actions use 20px.
- Use 1.5px stroke beside 400-weight text and 2px beside 500–600-weight labels.
- Use outline icons by default and fill only to reinforce an active/selected state when the family supports a coherent pair.
- Do not mix icon libraries within one surface.
- Test every icon at its rendered size; do not arbitrarily scale a 24px glyph to a soft fractional grid.
- Direction-dependent chevrons and navigation arrows flip in RTL; checkmarks, microphones, clocks, and playback icons do not.
- Every icon-only button has an accessible name and tooltip.
- The play triangle is shifted approximately 1–2px toward its visual point when required for optical centering.

Core concepts:

| Action/type | Icon concept | Required adjacent text or accessible name |
|---|---|---|
| Text capture | text lines | “Text note” where type is otherwise ambiguous. |
| Screenshot | image frame | “Screenshot”. |
| Voice | microphone / waveform | “Voice note” or “Start recording”. |
| External open | arrow leaving square | “Open in default application”. |
| Search | magnifier | Visible “Search” label or search-input label. |
| Context | folder/code repository | Context name remains visible. |
| Branch | Git branch | Branch value remains visible; icon alone is insufficient. |

## 7. Component Specifications

### Quick-capture popup

#### Structure

```text
┌──────────────────────────────────────────────────────┐
│ [context icon] stipen  /  feature/auth          [⌄] │
│                                                      │
│ Type or paste anything…                              │
│                                                      │
│ [image preview / voice controls when present]        │
│                                                      │
│ [Screenshot] [Voice]              Shift+Enter ↵  Enter│
└──────────────────────────────────────────────────────┘
```

- Context row: 12px text, one line, 8px vertical/14px horizontal inset.
- Context name uses primary text; branch uses muted text and a separator.
- Text area: 15px/22px, visually borderless inside the popup, minimum three lines.
- Placeholder: “Type or paste anything…”; it is not a label substitute for assistive technology.
- Bottom action row is secondary. Screenshot and Voice are quiet/ghost controls, not competing primary buttons.
- “Enter to save” may be shown as a keyboard hint; saving still uses actual keyboard behavior and an accessible Save control when pointer users need it.
- The context row is always an operable control. Missing, ambiguous, or stale context expands an inline chooser without discarding or covering entered content.
- Save error appears inline above the action row with a concise message and Retry; focus moves to the error summary only when required to make it discoverable.

The popup MUST NOT contain tags, folders, statuses, branch selection, formatting controls, search, or Library navigation.

#### Focus order

1. Text/caption input on open.
2. Screenshot action.
3. Voice action or recording controls.
4. Context control; opening it moves focus into the chooser.
5. Save and cancel affordances when exposed.

`Esc` cancels only when no child menu/chooser needs to close first. The first `Esc` closes the topmost child surface; the next cancels the capture.

### Context indicator and chooser

- Indicator uses a repository/folder icon, context name, separator, and branch.
- It is a compact button/combobox control, not a colorful badge. Its accessible name includes context and branch, followed by “Change context”.
- Resolved state is neutral. Ambiguous state uses a warning icon plus “Choose session”; stale state says “Session changed — choose again”.
- The chooser uses an accessible combobox/listbox pattern, supports typeahead, and separates “Live sessions” from “Saved contexts”. “Create context” follows matching saved items.
- Each live row shows application, project/worktree label, and branch when known. The exact pre-popup source may carry the text label “Current window”; do not communicate it by color alone.
- Distinguish VS Code windows, integrated terminals, external terminals, and shells by icon plus visible type label. A coding agent is represented only through its terminal/workspace—not by guessed agent identity.
- Never show terminal commands/output, editor content, agent conversation text, process IDs, raw correlation tokens, or full private paths.
- Selecting any option closes the chooser, restores focus to the context control, changes only the current capture context, and preserves text, preview, staged-media identity, recording state, and scroll position.
- If a live source becomes stale during selection or save, keep the chooser available and draft intact; never jump silently to another recent source.
- Project and standalone saved contexts are expressed by icon and text grouping, not color alone.
- Long values use middle truncation, with full accessible label and tooltip.

### Buttons

#### Primary button

- Background `--color-accent`, foreground `--color-on-accent`.
- Height 32px compact / 36px standard.
- Horizontal padding 12px compact / 16px standard.
- Radius 6px.
- Font 13px, weight 500.
- Hover uses `--color-accent-hover`.
- Active state uses `scale: 0.96` over 100–150ms unless the button is marked static.
- Focus uses a 2px accent ring plus 2px surface offset.
- Disabled state does not scale and uses a clearly unavailable cursor/state.

#### Secondary button

- Surface background and `--color-input-border` structural outline.
- Hover uses `--color-surface-subtle`.
- Selected state uses `--color-accent-soft` plus text/icon and `aria-pressed` or `aria-selected`.

#### Ghost/icon button

- No resting box unless required for target visibility.
- Hover uses `--color-surface-subtle`.
- Minimum interactive target: 32 × 32px in dense desktop UI; use 44 × 44px where the window has room or touch input is expected.
- Icon-only variants require `aria-label` and tooltip.

Buttons with a trailing icon SHOULD use approximately 2px less padding on the icon side when equal padding appears optically unbalanced.

### Inputs and search

- Height: 34px for single-line fields.
- Border: 1px `--color-input-border` for structural clarity.
- Radius: 6px.
- Background: `--color-surface`.
- Focus: border changes to accent and receives a non-clipped ring.
- Invalid: error icon, error border, and visible message tied with `aria-describedby`; color alone is insufficient.
- Search uses native `type="search"`, visible or accessible label, clear button, and platform-appropriate shortcut hint.
- Do not animate input borders or labels on each keystroke beyond a ≤100ms color transition.

### Capture stream and rows

- Group by human-readable date (“Today”, “Yesterday”, then localized date).
- Each row is a semantic list item with a main open/detail action.
- Desktop row grid: 48px timestamp column, flexible content, optional 120px metadata/action column.
- Row padding: 12px vertical, 8px horizontal.
- Use a divider between rows; do not wrap every item in an isolated card.
- Hover changes background in ≤100ms; selected state adds accent-soft background and a 2px leading accent bar.
- Show content first, then context/branch metadata.
- Text body shows up to a deliberate preview length with natural wrapping; do not produce an AI summary.
- Screenshot rows use a small 64–96px preview with the required neutral outline.
- Voice rows show waveform/progress, duration in tabular numerals, and caption when present.
- Row actions appear on focus-within as well as hover so keyboard users can discover them.

### Screenshot preview

- Preserve aspect ratio with `object-fit: contain`; never crop code or terminal content by default.
- Radius: 8px, with the neutral inside outline.
- Use a blurred neutral placeholder or skeleton based on dimensions, not a content-derived remote thumbnail.
- Provide descriptive alternative text from the user's caption when present. Otherwise use functional text such as “Screenshot captured in [context]”; do not pretend generated metadata describes image contents.
- Full preview supports zoom and external open without changing the canonical file.

### Voice player

- Play/pause is a native button with accessible state text.
- Timeline is keyboard operable when seeking is supported; otherwise expose progress without suggesting it is interactive.
- Always show elapsed and total duration with tabular numerals.
- Recording state includes text “Recording”, a red dot, elapsed time, and an input-level meter marked decorative unless it conveys actionable clipping.
- Stopping recording replaces the record control with playback and re-record options using both icon and text.
- Playback failure retains the file and shows a retryable inline error.

### Navigation

- Library navigation is a restrained 208px rail at wide widths.
- Items are 32–36px high with icon and label.
- Active state uses accent-soft background, semibold text, and `aria-current="page"`; it is never color alone.
- Projects and standalone contexts are separate labeled groups.
- Counts, if present, are muted tabular numerals and do not become notification badges.

### Menus, dialogs, and notifications

- Prefer anchored menus for compact choices and native/semantic `<dialog>` behavior for blocking confirmation.
- Trap focus only in modal dialogs, restore it to the invoker on close, and let `Esc` close the topmost transient surface.
- Use non-blocking status text for durable save; the normal success behavior is popup dismissal, not a toast.
- Errors that prevent save remain in the popup. Background enrichment failure belongs in capture detail/settings, not as a disruptive global alert.
- Any destructive future action requires explicit confirmation with the affected item named.

## 8. Application States

Every view MUST define these states; blank space or an endless spinner is not acceptable.

| State | Required treatment |
|---|---|
| Initial loading | Use a stable skeleton only if loading exceeds one animation frame; quick capture should usually render immediately. |
| Empty Library | “Nothing captured yet” plus the configured shortcut and a brief explanation; no stock illustration required. |
| Empty context | Name the context and explain that new captures assigned here will appear. |
| Empty search | Echo the query safely and offer to clear filters; do not imply semantic interpretation. |
| Save in progress | Prevent duplicate submit, preserve content, and use quiet progress on the Save affordance; never wait on enrichment. |
| Save failed | Keep content, show concise error + Retry, and do not dismiss. |
| Context ambiguous | Keep input ready; mark the context control with “Choose session” and open the grouped chooser on activation. |
| Context source stale | Keep all draft/media/recording state; say “Session changed — choose again” and refresh the live list. |
| Missing media | Preserve metadata and show “Media file unavailable” with non-destructive diagnostic action. |
| Microphone denied | Explain the permission, keep other capture types usable, and expose an OS-settings action when available. |
| Model absent | Describe transcription as optional and local; saving remains primary. |
| Enrichment pending | Small neutral status in detail view only. |
| Enrichment failed | Muted “Caption not generated” state; user caption entry remains available when supported. |

Skeletons use the same geometry as final content and respect reduced motion. Indeterminate spinners are reserved for compact commands whose final geometry is not content-like.

## 9. Motion and Interaction

### Motion tokens

```css
--duration-instant: 0ms;
--duration-fast: 100ms;
--duration-base: 150ms;
--duration-slow: 240ms;
--ease-out: cubic-bezier(0.2, 0, 0, 1);
--ease-in-out: cubic-bezier(0.4, 0, 0.2, 1);
```

Rules:

- Quick capture is a high-frequency interaction. Do not stage or stagger its entrance; the input appears ready immediately.
- Successful dismissal MAY use opacity plus at most `translateY(-4px)` for ≤100ms, but the window MUST hide immediately when reduced motion is active and MUST never exceed the 150ms product budget.
- Row hover, focus, and selection use background/color changes of 100ms or less.
- Use CSS transitions for interactive states so they remain interruptible.
- Keyframes are limited to one-shot skeleton/indeterminate progress treatments.
- Never use `transition: all`; list exact properties.
- Use `will-change` only after profiling shows first-frame stutter, and only for `transform`, `opacity`, or `filter`.
- Motion is never the only state signal.

Contextual icon swaps—such as play to pause—use this exact transition when motion is enabled:

```text
scale 0.25 → 1
opacity 0 → 1
blur 4px → 0
duration 300ms
bounce 0
```

If no motion library is already installed, keep both icons in the DOM and cross-fade with CSS using `cubic-bezier(0.2, 0, 0, 1)`. Do not add a motion dependency only for icon transitions. Default-state icons MUST NOT animate on initial render.

```css
@media (prefers-reduced-motion: reduce) {
  *, *::before, *::after {
    scroll-behavior: auto !important;
    animation-duration: 0.01ms !important;
    animation-iteration-count: 1 !important;
    transition-duration: 0.01ms !important;
  }
}
```

## 10. Page-Level Patterns

### Quick capture

The layout follows the action sequence, not a generic form:

1. quiet context strip;
2. immediately focused thought surface;
3. media preview/recording controls only when invoked;
4. compact actions and keyboard hints;
5. inline context resolution or error when required.

No save-success screen is shown. Durable success returns the user to the previous application.

### Library — Recent and All captures

```text
┌───────────────┬──────────────────────────┬─────────────────────┐
│ Navigation    │ Chronological stream     │ Detail (wide only)  │
│ Recent        │ Today                    │ Capture content     │
│ All captures  │ 22:14  text…             │ Metadata            │
│ Projects      │ 22:08  screenshot…       │ Media actions       │
│ Contexts      │ 21:37  voice…            │                     │
└───────────────┴──────────────────────────┴─────────────────────┘
```

- The stream remains the visual center.
- Detail is a third pane only when at least 900px is available.
- Opening detail at narrower widths replaces the stream and preserves Back focus/history.
- Date headers are sticky only if they do not obscure keyboard focus or reduce the stream excessively.

### Search

- Search field sits at the top of the stream, not on a separate dashboard.
- Results retain normal capture-row styling and chronology metadata.
- Match snippets highlight with `--color-accent-soft` plus appropriate text contrast; use `<mark>` semantics.
- Filters appear after the query and collapse into an accessible menu at narrow widths.
- Do not expose raw FTS syntax or imply semantic AI search.

### Projects and standalone contexts

- Use the same capture stream component for both.
- Project header shows project name and optional branch filter.
- Standalone context header does not reserve empty Git metadata space.
- Branch selection is a Library filter only and never appears in quick capture by default.

### Settings

- Use a single readable column, maximum 680px.
- Group: Capture shortcut, Context detection, Appearance, Local intelligence.
- Each setting has a visible label, current value, concise consequence, and inline error state.
- Local intelligence copy states clearly that transcription is optional, local, and never blocks saving.
- Model download displays byte progress and verification state without implying that core Lyn is incomplete.

## 11. Accessibility Contract

Lyn targets WCAG 2.1 AA for the WebView interface and native platform conventions where they are stronger.

### Keyboard

- Every action uses a semantic `<button>`, input, link, dialog, listbox, or equivalent accessible primitive.
- Tab order follows visual order and does not enter hidden controls.
- The quick-capture keyboard contract is preserved: `Enter` save, `Shift + Enter` newline, `Esc` cancel/topmost-close.
- Enter during IME composition MUST NOT save.
- Space activates buttons without scrolling.
- Visible focus is never removed.
- Focus returns to the prior application after successful save/cancel where the OS permits.
- Focus returns to the invoking control when a menu or dialog closes.

### Screen readers and semantics

- The capture input has a programmatic label even when the visual design relies on a placeholder.
- Context and branch are announced as separate values.
- Save errors use `role="alert"` only when immediate interruption is necessary; background statuses use polite live regions.
- Capture streams use lists with real headings for date groups.
- Icon-only buttons have concise accessible names.
- Images use caption-derived alternative text without claiming knowledge of unseen content.
- Recording elapsed time updates are throttled so live announcements do not fire every second; announce start, stop, and failure states.

### Pointer and target size

- Dense desktop controls have at least a 32 × 32px pointer target.
- Primary and touch-relevant actions target 44 × 44px when window geometry permits.
- Adjacent icon buttons have at least 4px visual separation and non-overlapping hit regions.

### Contrast and non-color cues

- Validate all token pairs in light and dark modes with automated contrast tests.
- Error, warning, selection, recording, missing-media, and capture-type states include text/icon/shape in addition to color.
- Screenshot outlines use neutral black/white to remain visible across arbitrary image colors.

### Zoom, reflow, and text

- UI remains usable at 200% text zoom without clipped primary actions.
- At narrow widths, panes reflow rather than force horizontal scrolling.
- Do not disable OS font smoothing, text selection in capture bodies, or user zoom solely for aesthetics.

### Reduced motion

- Honor `prefers-reduced-motion` for every custom animation.
- No workflow depends on animation completion.
- Recording and saving states remain visible without movement.

## 12. Language and Tone

### Voice

Use short, direct, calm language. Lyn is a tool, not a character performing enthusiasm.

Preferred:

- “Saved”
- “Choose a context to save this capture.”
- “Choose the session for this capture.”
- “Session changed — choose again.”
- “Microphone access is off.”
- “Media file unavailable.”
- “Caption not generated. Your voice note is saved.”
- “Transcription runs locally and never blocks saving.”

Avoid:

- “Awesome! Your brilliant thought has been captured!”
- “AI magic is working…”
- “Oopsie!”
- “Something went wrong” without a recoverable action.
- language suggesting cloud upload, account sync, or image understanding that does not exist.

### Capitalization

- Sentence case for titles, buttons, labels, and errors.
- Preserve user-entered project and branch casing.
- Use “Library” when naming the product area; use lowercase for generic captures, contexts, and settings.

## 13. Implementation Guidance

- Build lightweight Svelte primitives and CSS custom properties; do not import a heavyweight UI framework for basic controls.
- Prefer composition: `CapturePopup`, `ContextIndicator`, `ContextSourceChooser`, `CaptureStream`, `CaptureRow`, `MediaPreview`, `VoicePlayer`, and focused primitives.
- Keep data/IPC clients separate from presentational components.
- Colocate component tests and state stories/examples where the project tooling supports them.
- Use real capture examples in development fixtures; placeholder lorem ipsum hides wrapping and density defects.
- Lazy-load screenshot/audio detail and keep stream rows bounded.
- Preserve semantic HTML before reaching for ARIA.
- Any headless primitive must be verified for Svelte 5 compatibility, focus management, bundle cost, and Tauri WebView behavior before adoption.
- Do not add a global client-state library until local and feature-level Svelte state proves insufficient.

## 14. Visual Verification Checklist

Before a UI slice is considered verified:

- [ ] Quick capture accepts keyboard input immediately and does not animate a staged entrance.
- [ ] `Enter`, `Shift + Enter`, `Esc`, IME composition, and missing-context focus paths work.
- [ ] Concurrent VS Code windows, integrated/external terminals, coding-agent working directories, and Git worktrees produce distinct, accurate, safely labeled choices.
- [ ] The pre-popup foreground source is marked and selected ahead of unrelated recent activity; ambiguity never guesses.
- [ ] Switching or losing a live source preserves text, staged media, recording state, scroll, and focus recovery.
- [ ] Chooser accessibility works with keyboard and screen readers, including group labels, current-window status, ambiguity, and stale-source errors.
- [ ] No terminal/editor/agent content, process ID, correlation token, or full private path appears in the UI, DOM, accessibility tree, or event payloads.
- [ ] All interactive elements are reachable and usable by keyboard.
- [ ] Focus indicators remain visible in light and dark themes.
- [ ] Screen-reader structure, labels, live regions, and error associations are correct.
- [ ] Automated contrast checks pass for text, focus, input, status, and selected-state token pairs.
- [ ] Library layouts work at 320px, 768px, 1024px, and 1440px; quick capture works at 360–680px.
- [ ] Text zoom at 200% does not hide save, cancel, context resolution, or media controls.
- [ ] Loading, empty, save-failed, permission-denied, missing-media, and enrichment states are rendered.
- [ ] Screenshot edges use pure-black/pure-white 10% outlines, including arbitrary bright and dark images.
- [ ] Icons remain crisp at 16px/20px and their stroke weight matches adjacent text.
- [ ] Hover, focus, active, selected, disabled, recording, playback, and error states are inspected.
- [ ] Motion is replayed slowly during browser inspection; transitions remain interruptible and restrained.
- [ ] Reduced-motion mode removes non-essential motion without removing state cues.
- [ ] No `transition: all`, broad `will-change`, remote font request, decorative gradient, or mixed icon family is present.
- [ ] Browser console has no runtime errors and accessibility tooling reports no actionable violations.

This checklist verifies implementation behavior. Completion of this document alone is documentary acceptance, not proof that the UI has been implemented or tested.
