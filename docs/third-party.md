# Third-party data

Aion Tallymark is Copyright (C) 2026 Shiro, licensed GPL-3.0-or-later. This file lists the third-party material it includes.

## English NPC names

- File in this repository: `data/npc-names-en.json` (unmodified)
- Source: [A2Tools DPS Meter](https://github.com/taengu/A2Tools-DPS-Meter), file `src/data/i18n/npcs/en.json`
- Source commit: `bd878f6c9b1ce55c589724d2fb976c1eef0de8d0`
- Retrieved: 2026-11-02
- License: GPL-3.0

The file is bundled under GPL-3.0, which is compatible with this project's GPL-3.0-or-later license. It is compiled into the engine with `include_str!`; the app reads only the `name`, `isBoss` and `isDummy` fields and never downloads it or any update to it.

## English skill names

- File in this repository: `data/skills_en.json`
- Source: [A2Tools DPS Meter](https://github.com/taengu/A2Tools-DPS-Meter) English skill table
- License: GPL-3.0

## Code

The packet framing and the damage-record layout are derived from A2Tools DPS Meter (GPL-3.0), Copyright (c) 2026 taengu.

A2Tools notes that portions of it are derived from [AION2-DPS-Meter](https://github.com/taengu/AION2-DPS-Meter), originally released under the MIT License, Copyright (c) 2026 TK-open-public. The MIT License text, which covers those portions:

```text
MIT License

Copyright (c) 2026 TK-open-public

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

## Font

Inter, SIL Open Font License 1.1, see `ui/fonts/Inter-LICENSE.txt`.