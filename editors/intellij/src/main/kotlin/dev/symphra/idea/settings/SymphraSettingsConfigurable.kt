package dev.symphra.idea.settings

import com.intellij.openapi.options.BoundConfigurable
import com.intellij.openapi.ui.DialogPanel
import com.intellij.ui.dsl.builder.bindText
import com.intellij.ui.dsl.builder.panel

class SymphraSettingsConfigurable : BoundConfigurable("Symphra") {
    override fun createPanel(): DialogPanel {
        val settings = SymphraSettingsState.getInstance()
        return panel {
            row("Language server path:") {
                textField()
                    .bindText(settings::serverPath)
                    .comment(
                        "Path to the symphra-lsp executable. When empty, the plugin looks for " +
                            "target/debug/symphra-lsp or target/release/symphra-lsp under the project, " +
                            "then falls back to symphra-lsp on PATH.",
                    )
                    .resizableColumn()
            }
        }
    }
}
