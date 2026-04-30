# ClawAgentMonitor

[![CI](https://github.com/KarmaGame33/clawagentmonitor/actions/workflows/ci.yml/badge.svg?branch=main)](https://github.com/KarmaGame33/clawagentmonitor/actions/workflows/ci.yml)

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

- Linux (seule plateforme supportée pour l'instant ; développé sur CachyOS / Arch)

Le code est écrit en Rust + Slint, qui supportent nativement macOS et Windows. Un portage est techniquement possible et préparé (le module `tray` est `cfg(target_os = "linux")`-gated pour ne pas bloquer un build cross-OS), mais pas testé ni livré pour l'instant.

## Licence

MIT
