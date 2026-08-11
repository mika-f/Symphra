package dev.symphra.idea.lexer

import com.intellij.psi.tree.IElementType
import dev.symphra.idea.SymphraLanguage

class SymphraTokenType(debugName: String) : IElementType(debugName, SymphraLanguage) {
    override fun toString(): String = "SymphraTokenType.${super.toString()}"
}

object SymphraTokenTypes {
    @JvmField
    val COMMENT = SymphraTokenType("COMMENT")

    @JvmField
    val STRING = SymphraTokenType("STRING")

    @JvmField
    val NUMBER = SymphraTokenType("NUMBER")

    @JvmField
    val RATE_UNIT = SymphraTokenType("RATE_UNIT")

    @JvmField
    val PITCH = SymphraTokenType("PITCH")

    @JvmField
    val KEYWORD = SymphraTokenType("KEYWORD")

    @JvmField
    val IDENTIFIER = SymphraTokenType("IDENTIFIER")

    @JvmField
    val BRACE = SymphraTokenType("BRACE")

    @JvmField
    val OPERATOR = SymphraTokenType("OPERATOR")

    // Keywords reserved by TokenKind::keyword in crates/symphra-syntax/src/token.rs.
    // Values such as `major`, `khz`, `stereo`, `sine`, `triangle`, and `db` are
    // lexed as plain identifiers.
    @JvmField
    val KEYWORDS = setOf(
        "project", "song", "seed", "sample_rate", "output", "tempo", "meter", "key",
        "instrument", "sample", "choose", "weight", "sampled", "sampler", "drum_machine",
        "soundfont", "preset", "source", "root", "pack", "bank", "drum", "rhythm",
        "resolution", "hit", "track", "role", "volume", "layer", "use", "play",
        "trigger_with", "gate", "transpose", "gain", "repeat", "reverse", "pan",
        "alternate", "chance", "speed", "retrigger", "choose_sample", "at", "pattern",
        "arrangement", "with", "sequence", "steps", "step", "degree", "octave", "note", "chord",
        "rest", "for", "velocity", "bar", "effect", "delay", "mix", "time", "feedback",
        "filter", "cutoff", "resonance", "reverb", "size", "automate", "lfo", "range",
        "rate", "cycles", "section", "bars", "parallel", "exact", "master", "limiter",
        "ceiling", "synth", "supersaw", "envelope", "attack", "decay", "sustain",
        "release", "voices", "detune", "spread"
    )

    // Natural (C4), sharp (C#4), flat (Cb4), and negative-octave (C-1, C#-1, Cb-1) pitches.
    @JvmField
    val PITCH_REGEX = Regex("^[A-G](#|b)?-?[0-9]+$")
}
