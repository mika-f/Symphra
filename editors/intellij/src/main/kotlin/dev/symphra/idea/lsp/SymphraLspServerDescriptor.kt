package dev.symphra.idea.lsp

import com.intellij.execution.configurations.GeneralCommandLine
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.platform.lsp.api.ProjectWideLspServerDescriptor
import com.intellij.platform.lsp.api.customization.LspFormattingSupport
import dev.symphra.idea.settings.SymphraSettingsState
import java.io.File

private val BINARY_NAME = if (System.getProperty("os.name").lowercase().contains("win")) "symphra-lsp.exe" else "symphra-lsp"

class SymphraLspServerDescriptor(project: Project) : ProjectWideLspServerDescriptor(project, "Symphra") {
    // Symphra has no IntelliJ-native formatter (no registered FormattingModelBuilder
    // for .sym files), so the default LspFormattingSupport already delegates
    // "Reformat Code" to the language server. Without this override,
    // lspFormattingSupport defaults to null and the IDE never sends
    // textDocument/formatting at all.
    override val lspFormattingSupport = LspFormattingSupport()

    override fun isSupportedFile(file: VirtualFile): Boolean = file.extension == "sym"

    override fun createCommandLine(): GeneralCommandLine =
        GeneralCommandLine(resolveServerCommand(project))

    private fun resolveServerCommand(project: Project): String {
        val configured = SymphraSettingsState.getInstance().serverPath
        if (configured.isNotBlank()) {
            return configured
        }
        findWorkspaceBinary(project)?.let { return it }
        return BINARY_NAME
    }

    private fun findWorkspaceBinary(project: Project): String? {
        val basePath = project.basePath ?: return null
        for (profile in listOf("release", "debug")) {
            val candidate = File(basePath, "target/$profile/$BINARY_NAME")
            if (candidate.exists()) {
                return candidate.absolutePath
            }
        }
        return null
    }
}
