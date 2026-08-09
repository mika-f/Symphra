package dev.symphra.idea.highlighting

import com.intellij.openapi.fileTypes.SyntaxHighlighter
import com.intellij.openapi.options.colors.AttributesDescriptor
import com.intellij.openapi.options.colors.ColorDescriptor
import com.intellij.openapi.options.colors.ColorSettingsPage
import dev.symphra.idea.SymphraFileType
import javax.swing.Icon

class SymphraColorSettingsPage : ColorSettingsPage {
    override fun getDisplayName(): String = "Symphra"

    override fun getIcon(): Icon = SymphraFileType.icon

    override fun getHighlighter(): SyntaxHighlighter = SymphraSyntaxHighlighter()

    override fun getDemoText(): String = """
        project {
          seed 20260809
          sample_rate 48khz
          output stereo
        }

        song "First Song" {
          tempo 150bpm
          meter 4/4
          key C major

          pattern melody = sequence {
            note C4 for 1/4
            rest for 1/4
            chord C4 E4 G4 for 1/4
            note G4 for 1/2
          }

          arrangement { melody }
        }
    """.trimIndent()

    override fun getAdditionalHighlightingTagToDescriptorMap(): MutableMap<String, com.intellij.openapi.editor.colors.TextAttributesKey>? = null

    override fun getAttributeDescriptors(): Array<AttributesDescriptor> = arrayOf(
        AttributesDescriptor("Comment", SymphraHighlightingColors.COMMENT),
        AttributesDescriptor("String", SymphraHighlightingColors.STRING),
        AttributesDescriptor("Number", SymphraHighlightingColors.NUMBER),
        AttributesDescriptor("Rate unit (khz, bpm)", SymphraHighlightingColors.RATE_UNIT),
        AttributesDescriptor("Pitch (C4, A4)", SymphraHighlightingColors.PITCH),
        AttributesDescriptor("Keyword", SymphraHighlightingColors.KEYWORD),
        AttributesDescriptor("Identifier", SymphraHighlightingColors.IDENTIFIER),
        AttributesDescriptor("Braces", SymphraHighlightingColors.BRACE),
        AttributesDescriptor("Operator", SymphraHighlightingColors.OPERATOR),
    )

    override fun getColorDescriptors(): Array<ColorDescriptor> = ColorDescriptor.EMPTY_ARRAY
}
