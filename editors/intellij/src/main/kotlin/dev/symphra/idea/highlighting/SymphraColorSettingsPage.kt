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

          instrument lead = sampler {
            pack "lead"
          }

          instrument piano = soundfont {
            source "gm.sf2"
            preset "Acoustic Grand Piano"
          }

          instrument pad = synth supersaw {
            voices 5
            detune 0.28
            spread 0.75
            envelope {
              attack 8ms
              decay 120ms
              sustain 0.72
              release 180ms
            }
          }

          rhythm stabs resolution 1/8 {
            hit rest hit hit
          }

          pattern melody = sequence {
            note C4 for 1/4
            rest for 1/4
            chord C4 E4 G4 for 1/4 velocity 96
            note G4 for 1/2
          }

          track lead_track role harmony {
            instrument lead
            volume -5.2db

            play melody
              |> trigger_with stabs
              |> gate 95%
              |> pan alternate(30%, 70%)
              |> chance 15% {
                   transpose +12st
                 }
              |> choose_sample 0..3

            effect filter {
              cutoff 2000hz
              resonance 0.40
            }

            automate cutoff {
              lfo sine {
                range 600hz..2800hz
                rate 2 cycles/bar
              }
            }

            effect reverb {
              mix 0.40
              size 0.80
            }
          }

          section phrase bars 4 {
            parallel exact {
              play track lead_track
            }
          }

          arrangement {
            play phrase
          }

          master {
            limiter {
              ceiling -0.3db
            }
          }
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
