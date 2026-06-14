# Context Map

## Contexts

- [Arena](./src/arena/CONTEXT.md) — the custom bot-battle game: hosts matches and runs participants' bots against each other.
- [Retreat](./retreat/CONTEXT.md) — the event itself: shared language for running the day (Marketplace, Sessions, the optional Brownfield Swap).

## Relationships

- **Retreat → Arena**: the Arena is one **Track** a team can pick at the Marketplace; its end-of-day tournament feeds the Retro/Demo.
