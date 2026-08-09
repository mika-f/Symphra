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

    // Keywords reserved by the Draft 0.1 grammar (docs/language/draft-0.1.md).
    // Values such as `major`, `khz`, and `stereo` are lexed as plain identifiers.
    @JvmField
    val KEYWORDS = setOf(
        "project", "song", "seed", "sample_rate", "output", "tempo", "meter", "key",
        "instrument", "sample", "choose", "weight", "sampled", "sampler", "source", "root",
        "pack", "rhythm", "resolution", "hit", "track", "role", "play", "trigger_with", "gate",
        "transpose", "pattern", "arrangement", "with", "sequence", "steps", "degree", "octave",
        "note", "chord", "rest", "for", "velocity"
    )

    // Natural (C4), sharp (C#4), flat (Cb4), and negative-octave (C-1, C#-1, Cb-1) pitches.
    @JvmField
    val PITCH_REGEX = Regex("^[A-G](#|b)?-?[0-9]+$")
}
