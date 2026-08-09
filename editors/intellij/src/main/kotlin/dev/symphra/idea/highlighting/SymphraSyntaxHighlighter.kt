package dev.symphra.idea.highlighting

import com.intellij.lexer.Lexer
import com.intellij.openapi.editor.DefaultLanguageHighlighterColors
import com.intellij.openapi.editor.HighlighterColors
import com.intellij.openapi.editor.colors.TextAttributesKey
import com.intellij.openapi.editor.colors.TextAttributesKey.createTextAttributesKey
import com.intellij.openapi.fileTypes.SyntaxHighlighterBase
import com.intellij.psi.TokenType
import com.intellij.psi.tree.IElementType
import dev.symphra.idea.lexer.SymphraLexer
import dev.symphra.idea.lexer.SymphraTokenTypes

object SymphraHighlightingColors {
    @JvmField
    val COMMENT: TextAttributesKey = createTextAttributesKey("SYMPHRA_COMMENT", DefaultLanguageHighlighterColors.LINE_COMMENT)

    @JvmField
    val STRING: TextAttributesKey = createTextAttributesKey("SYMPHRA_STRING", DefaultLanguageHighlighterColors.STRING)

    @JvmField
    val NUMBER: TextAttributesKey = createTextAttributesKey("SYMPHRA_NUMBER", DefaultLanguageHighlighterColors.NUMBER)

    @JvmField
    val RATE_UNIT: TextAttributesKey = createTextAttributesKey("SYMPHRA_RATE_UNIT", DefaultLanguageHighlighterColors.KEYWORD)

    @JvmField
    val PITCH: TextAttributesKey = createTextAttributesKey("SYMPHRA_PITCH", DefaultLanguageHighlighterColors.CONSTANT)

    @JvmField
    val KEYWORD: TextAttributesKey = createTextAttributesKey("SYMPHRA_KEYWORD", DefaultLanguageHighlighterColors.KEYWORD)

    @JvmField
    val IDENTIFIER: TextAttributesKey = createTextAttributesKey("SYMPHRA_IDENTIFIER", DefaultLanguageHighlighterColors.IDENTIFIER)

    @JvmField
    val BRACE: TextAttributesKey = createTextAttributesKey("SYMPHRA_BRACE", DefaultLanguageHighlighterColors.BRACES)

    @JvmField
    val OPERATOR: TextAttributesKey = createTextAttributesKey("SYMPHRA_OPERATOR", DefaultLanguageHighlighterColors.OPERATION_SIGN)

    @JvmField
    val BAD_CHARACTER: TextAttributesKey = createTextAttributesKey("SYMPHRA_BAD_CHARACTER", HighlighterColors.BAD_CHARACTER)
}

class SymphraSyntaxHighlighter : SyntaxHighlighterBase() {
    override fun getHighlightingLexer(): Lexer = SymphraLexer()

    override fun getTokenHighlights(tokenType: IElementType?): Array<TextAttributesKey> {
        val key = when (tokenType) {
            SymphraTokenTypes.COMMENT -> SymphraHighlightingColors.COMMENT
            SymphraTokenTypes.STRING -> SymphraHighlightingColors.STRING
            SymphraTokenTypes.NUMBER -> SymphraHighlightingColors.NUMBER
            SymphraTokenTypes.RATE_UNIT -> SymphraHighlightingColors.RATE_UNIT
            SymphraTokenTypes.PITCH -> SymphraHighlightingColors.PITCH
            SymphraTokenTypes.KEYWORD -> SymphraHighlightingColors.KEYWORD
            SymphraTokenTypes.IDENTIFIER -> SymphraHighlightingColors.IDENTIFIER
            SymphraTokenTypes.BRACE -> SymphraHighlightingColors.BRACE
            SymphraTokenTypes.OPERATOR -> SymphraHighlightingColors.OPERATOR
            TokenType.BAD_CHARACTER -> SymphraHighlightingColors.BAD_CHARACTER
            else -> return TextAttributesKey.EMPTY_ARRAY
        }
        return arrayOf(key)
    }
}
