---
name: aello-tui-design
description: "aello's TUI visual style — \"Kinetic Command\" palette and conventions"
metadata: 
  node_type: memory
  type: reference
  originSessionId: afdce7ab-1496-4a48-a656-d27d377c3496
---

aello's TUI follows the "Kinetic Command" design system (futuristic-brutalist terminal; data_terminal screen = sidebar + central data table is the blueprint-registry template). Palette as `Color::Rgb` consts in `src/tui.rs`: BG `#0a0a0a`, surface `#141313`, stripe `#111111`; primary kinetic-orange `#ffb596`, hot-orange `#ff6600`, amber `#ffae00`; text `#e5e2e1`, muted `#aa8a7d`, dim `#5a4136`, error `#ffb4ab`, success-green `#4aff8a`. Conventions: UPPERCASE labels, sharp 1px bordered modules (dim grey, active=orange), data tables w/ orange-underlined header + alternating row tint + no vertical lines, telemetry decorations in corners/footer, block cursor `█`. Centered modals (not bottom prompts), pick-from-list not typing. Add flow: name → model → persona → caps checklist. Truecolor needed; degrades on 256-color SSH. [[aello-overview]]
