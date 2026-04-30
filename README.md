# ClawAgentMonitor

Application desktop cross-platform pour monitorer l'état des agents [OpenClaw](https://docs.openclaw.ai/) en temps réel, avec watchdog optionnel pour le Gateway.

## Statut

En cours de développement (scaffolding initial).

## Stack

- Rust 1.95+
- [Slint UI](https://slint.dev) pour l'interface graphique
- Source de données : `openclaw status --all --json` + `openclaw gateway probe`

## Build

```bash
cargo build --release
cargo run -p clawagentmonitor
```

## Plateformes ciblées

- Linux (premier livrable, développé sur CachyOS / Arch)
- macOS (à venir)
- Windows (à venir)

## Licence

MIT
