---
name: translate-strudel-to-symphra
description: Translate and adapt Strudel or Tidal-style JavaScript compositions and Mini-Notation into valid Symphra (`.sym`) songs. Use when porting Strudel patterns, cycles, stacks, arrangements, instruments, effects, modulation, or live-coding sketches to Symphra while preserving musical intent and explicitly documenting approximations and unsupported behavior. Requires Symphra-side language knowledge from the `symphra` skill.
---

# Translate Strudel to Symphra

Translate musical behavior, not JavaScript syntax. Strudel describes recurring cycle-based patterns and live transformations; Symphra describes an explicit, finite song rendered offline. Reconstruct the source's audible intent as Symphra material, tracks, sections, and arrangement.

## Load the required knowledge

1. Use the `symphra` skill for current target syntax, capabilities, and validation rules. When both skills are adjacent on disk, read `../symphra/SKILL.md` and its referenced files as needed.
2. Read [references/mapping.md](references/mapping.md) before translating.
3. For unfamiliar or recently changed Strudel behavior, verify it against the official Strudel documentation rather than guessing.

## Follow the translation workflow

### 1. Inventory the source

Identify the tempo and cycle/bar relationship; named patterns and lengths; musical events and pattern operations; sound sources, effects, and modulation; form-building operations; and required external assets.

Do not assume one Strudel cycle equals one 4/4 bar. Infer the relationship from `setcps`/`setcpm`, comments, pattern structure, and arrangement lengths. If it remains ambiguous and materially affects the result, state the chosen interpretation.

### 2. Create a translation ledger

Classify each meaningful source behavior before writing the target:

- **Exact**: Symphra can express the same musical behavior directly.
- **Approximate**: preserve the musical role with a documented substitute.
- **Unsupported**: omit only after explaining the audible loss and, when useful, a manual asset or arrangement workaround.

Keep this ledger concise. Put important approximations in comments near the relevant declaration or summarize them when handing off the file.

### 3. Normalize time and pitch

Expand Mini-Notation conceptually into an absolute timeline before choosing Symphra syntax. Preserve nested subdivision durations, rests, and simultaneous events. Resolve Strudel chord voicings and root-note helpers to explicit pitches or supported Symphra chord symbols; never assume both engines choose the same voicing.

Prefer `sequence step` for stable pitched grids, `steps` and subdivisions for percussion, `rhythm` plus `trigger_with` for `.struct(...)`, and derived patterns or `arpeggiate` only when their semantics match.

### 4. Rebuild performance and form

Map independent stacked voices to tracks played together in a section. Map `arrange([cycles, pattern], ...)` to named sections and a section arrangement after converting cycles to bars. Use `layer` for transformed copies of one musical line, such as an octave `superimpose`.

Choose instruments from assets that actually exist. Do not invent sample, SoundFont, preset, drum-bank, or VST3 paths. Prefer a disclosed built-in approximation when assets are unavailable.

### 5. Handle semantic gaps deliberately

Do not silently translate unsupported continuous modulation, random event deletion, effect chains, bus/orbit behavior, or unsupported synthesis. Select the closest stable musical result, simplify it, or ask for an asset only when that choice changes the requested outcome materially.

Treat numeric audio parameters as starting points, not guaranteed equivalents. Strudel and Symphra gain, resonance, reverb size, envelopes, and delay controls do not necessarily share scales or DSP implementations.

### 6. Validate the result

Follow the `symphra` skill's validation workflow. At minimum, format the generated `.sym`. Render it when all assets are available. Fix syntax, compiler, scheduling, and asset diagnostics before claiming a successful port.

Report the cycle-to-bar interpretation, required assets, material approximations or omissions, and validation results.

## Keep the boundary clear

Use this skill only for the translation process and Strudel-to-Symphra semantic mapping. Keep general Symphra language facts in the `symphra` skill.
