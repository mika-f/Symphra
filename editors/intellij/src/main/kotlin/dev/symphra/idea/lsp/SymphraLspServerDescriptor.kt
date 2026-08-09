package dev.symphra.idea.lsp

import com.intellij.execution.configurations.GeneralCommandLine
import com.intellij.openapi.project.Project
import com.intellij.openapi.vfs.VirtualFile
import com.intellij.platform.lsp.api.ProjectWideLspServerDescriptor
import dev.symphra.idea.settings.SymphraSettingsState
import java.io.File

private val BINARY_NAME = if (System.getProperty("os.name").lowercase().contains("win")) "symphra-lsp.exe" else "symphra-lsp"

class SymphraLspServerDescriptor(project: Project) : ProjectWideLspServerDescriptor(project, "Symphra") {
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
        for (profile in listOf("debug", "release")) {
            val candidate = File(basePath, "target/$profile/$BINARY_NAME")
            if (candidate.exists()) {
                return candidate.absolutePath
            }
        }
        return null
    }
}
