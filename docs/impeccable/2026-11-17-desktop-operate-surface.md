# rtools desktop — Operate surface brief

Scope: Windows desktop application, full daily workspace including tasks,
calendar review, transfer, synchronization, MCP, and settings.

Mode: Operate. The interface exists to help one person capture, scan, complete,
and maintain tasks with minimal interruption.

Audience and job: a single Windows 11 user managing a moderately long personal
daily list. The primary action is quick capture; the dominant recurring task is
scanning and completing items. Settings and transfer are lower-frequency tasks.

Success: the current task list is readable within seconds, common actions remain
close to each task, the window feels native to Windows 11, and visual decoration
never competes with task content.

Constraints: preserve every existing feature and command, preserve Mica and
material fallbacks, use the established Things 3 reference from
`docs/requirements.md`, and keep the 4/8px spacing vocabulary.

## Direction contract

THESIS: A quiet personal task desk whose calm, low-density list inherits the
existing Things 3 brief and refuses the nested-card dashboard pattern.

OWN-WORLD: Transparent Mica ground; one restrained system accent for navigation,
selection, and primary actions; semantic color only for priority and completion;
Segoe UI Variable with Microsoft YaHei UI fallback; hairline separators, selective
rounded surfaces, and one consistent icon language.

STORY: The user opens the app, immediately knows what is active, can add a task
without searching, and can complete or adjust it from the same calm row.

FIRST VIEWPORT: A native Win11 Mica window with a 224px navigation rail and a
single dominant daily list. Quick capture is a slim inline row above the list.
Task rows use whitespace and compact semantic dots rather than boxed cards.

FORM: Think-first desktop task tool; Operate mode; code-led. seed: none
(unavailable).

FINISH: unreviewed and undocumented is unfinished; this build ends with the
finish review, the verdict, DESIGN.md, and every shipping raster carrying its
provenance.
