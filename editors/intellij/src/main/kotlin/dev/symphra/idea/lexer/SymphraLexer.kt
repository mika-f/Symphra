package dev.symphra.idea.lexer

import com.intellij.lexer.LexerBase
import com.intellij.psi.TokenType
import com.intellij.psi.tree.IElementType

/**
 * A hand-written, eagerly-tokenizing lexer for the still-evolving Draft 0.1 grammar
 * (docs/language/draft-0.1.md). It mirrors the token classes highlighted by the VS Code
 * TextMate grammar (editors/vscode/syntaxes/symphra.tmLanguage.json) rather than a formal
 * BNF, since a PSI parser would need to track the grammar too closely while it is still
 * changing. Full semantic help comes from the symphra-lsp language server instead.
 */
class SymphraLexer : LexerBase() {
    private data class Token(val start: Int, val end: Int, val type: IElementType)

    private var buffer: CharSequence = ""
    private var rangeStart: Int = 0
    private var rangeEnd: Int = 0
    private var tokens: List<Token> = emptyList()
    private var index: Int = 0

    override fun start(buffer: CharSequence, startOffset: Int, endOffset: Int, initialState: Int) {
        this.buffer = buffer
        this.rangeStart = startOffset
        this.rangeEnd = endOffset
        this.tokens = tokenize(buffer, startOffset, endOffset)
        this.index = 0
    }

    override fun getState(): Int = 0

    override fun getTokenType(): IElementType? = tokens.getOrNull(index)?.type

    override fun getTokenStart(): Int = tokens.getOrNull(index)?.start ?: rangeEnd

    override fun getTokenEnd(): Int = tokens.getOrNull(index)?.end ?: rangeEnd

    override fun advance() {
        index++
    }

    override fun getBufferSequence(): CharSequence = buffer

    override fun getBufferEnd(): Int = rangeEnd

    private companion object {
        fun tokenize(text: CharSequence, start: Int, end: Int): List<Token> {
            val out = ArrayList<Token>()
            var i = start
            while (i < end) {
                val c = text[i]
                when {
                    c.isWhitespace() -> {
                        val s = i
                        while (i < end && text[i].isWhitespace()) i++
                        out += Token(s, i, TokenType.WHITE_SPACE)
                    }

                    c == '#' || (c == '/' && i + 1 < end && text[i + 1] == '/') -> {
                        val s = i
                        while (i < end && text[i] != '\n') i++
                        out += Token(s, i, SymphraTokenTypes.COMMENT)
                    }

                    c == '"' -> {
                        val s = i
                        i++
                        while (i < end && text[i] != '"') {
                            if (text[i] == '\\' && i + 1 < end) i++
                            i++
                        }
                        if (i < end) i++
                        out += Token(s, i, SymphraTokenTypes.STRING)
                    }

                    c.isDigit() -> {
                        val s = i
                        while (i < end && text[i].isDigit()) i++
                        if (i < end && text[i] == '.' && i + 1 < end && text[i + 1].isDigit()) {
                            i++
                            while (i < end && text[i].isDigit()) i++
                        }
                        out += Token(s, i, SymphraTokenTypes.NUMBER)
                        if (i < end && (text[i].isLetter() || text[i] == '_')) {
                            val us = i
                            while (i < end && (text[i].isLetterOrDigit() || text[i] == '_')) i++
                            out += Token(us, i, SymphraTokenTypes.RATE_UNIT)
                        }
                    }

                    c.isLetter() || c == '_' -> {
                        val s = i
                        while (i < end && (text[i].isLetterOrDigit() || text[i] == '_')) i++
                        // Mirrors Lexer::pitch_suffix in crates/symphra-syntax/src/lexer.rs: a `#`
                        // or `-` right after a bare pitch prefix (A-G, or that + `b`) belongs to the
                        // pitch when a digit (optionally signed) follows it. Otherwise `#` is left
                        // for the comment branch above, exactly like the real lexer.
                        if (isPitchPrefix(text, s, i)) {
                            if (i < end && text[i] == '#' && octaveFollows(text, i + 1, end)) i++
                            if (i < end && text[i] == '-' && octaveFollows(text, i, end)) i++
                            while (i < end && text[i].isDigit()) i++
                        }
                        val word = text.subSequence(s, i).toString()
                        val type = when {
                            word in SymphraTokenTypes.KEYWORDS -> SymphraTokenTypes.KEYWORD
                            SymphraTokenTypes.PITCH_REGEX.matches(word) -> SymphraTokenTypes.PITCH
                            else -> SymphraTokenTypes.IDENTIFIER
                        }
                        out += Token(s, i, type)
                    }

                    c == '{' || c == '}' -> {
                        out += Token(i, i + 1, SymphraTokenTypes.BRACE)
                        i++
                    }

                    c == '=' || c == '/' -> {
                        out += Token(i, i + 1, SymphraTokenTypes.OPERATOR)
                        i++
                    }

                    else -> {
                        out += Token(i, i + 1, TokenType.BAD_CHARACTER)
                        i++
                    }
                }
            }
            return out
        }

        /** True if `text[s..i)` is a bare pitch prefix: one of `A`-`G`, or that letter plus `b`. */
        fun isPitchPrefix(text: CharSequence, s: Int, i: Int): Boolean {
            val len = i - s
            if (len == 1) return text[s] in 'A'..'G'
            if (len == 2) return text[s] in 'A'..'G' && text[s + 1] == 'b'
            return false
        }

        /** True if an optional `-` followed by a digit starts at `from`. */
        fun octaveFollows(text: CharSequence, from: Int, end: Int): Boolean {
            var idx = from
            if (idx < end && text[idx] == '-') idx++
            return idx < end && text[idx].isDigit()
        }
    }
}
