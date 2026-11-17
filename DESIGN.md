# rtools design system

## Direction

rtools is a quiet personal task desk. Its visual identity keeps the existing
Things 3 reference from `docs/requirements.md`: low information density,
generous whitespace, restrained priority color, and task content ahead of
application chrome. The Windows implementation translates that identity into
native Win11 behavior with Mica and FluentAvalonia controls.

## Use scene

A single user works on a Windows 11 desktop during normal indoor use, often
keeping rtools open for quick capture and task completion. The app should
disappear into the task after a short scan.

## Color

- Color strategy: Restrained.
- Mica is the window ground. Navigation and content surfaces use translucent
  neutral layers so the desktop material remains present.
- The Windows system accent is reserved for primary actions, selection, and
  focus.
- High priority uses the red token, medium priority amber, and low priority a
  muted neutral from `packages/design-tokens/colors.json`.
- Completion uses the established green token. Status colors appear only in
  status text and inline notifications.

## Typography

- Family: `Segoe UI Variable Text`, falling back to `Microsoft YaHei UI`.
- Scale: 12 for captions and metadata, 14 for body and task titles, 18 for
  section titles, 24 for page titles.
- Use regular weight for task titles and semibold weight for headings and
  primary actions.

## Layout

- Base unit: 8px. Common gaps are 4, 8, 12, 16, 24, and 32px.
- Persistent navigation rail: 224px.
- Default window: 880x720; minimum supported size: 700x560.
- The task page uses one dominant reading column. Settings and transfer may
  use grouped sections, but cards do not nest.
- Page margins are 24px vertically and 30px horizontally.

## Components

- Navigation: FluentAvalonia `FANavigationView` with one icon language.
- Task row: native checkbox, 7px semantic priority dot, title, optional
  reminder, and compact icon actions. Rows use whitespace rather than borders.
- Quick capture: one inline control group followed by compact priority
  selection.
- Settings: FluentAvalonia `FASettingsExpander` and
  `FASettingsExpanderItem`, using disclosure instead of long stacked forms.
- Errors: FluentAvalonia `FAInfoBar`.
- Dialogs: reserved for protected editing and incoming-transfer confirmation.

## States and motion

- Every interactive control uses the FluentAvalonia default, hover, pressed,
  focus, disabled, and error states.
- Motion is limited to state feedback and native Fluent control transitions.
  No page-load choreography.
- Empty states explain the next action directly.

## Accessibility and resilience

- Mica falls back to Acrylic or a solid surface when Windows transparency is
  disabled or unavailable.
- Text remains at readable sizes in both light and dark variants.
- Icon-only task actions keep descriptive tooltips.
- Keyboard focus follows the native FluentAvalonia control behavior.
