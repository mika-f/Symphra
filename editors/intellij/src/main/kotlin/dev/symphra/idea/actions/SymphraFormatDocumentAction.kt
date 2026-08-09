package dev.symphra.idea.actions

import com.intellij.openapi.actionSystem.ActionUpdateThread
import com.intellij.openapi.actionSystem.AnAction
import com.intellij.openapi.actionSystem.AnActionEvent
import com.intellij.openapi.actionSystem.CommonDataKeys
import com.intellij.openapi.command.WriteCommandAction
import com.intellij.openapi.diagnostic.Logger
import com.intellij.openapi.project.Project
import dev.symphra.idea.SymphraFileType
import dev.symphra.idea.settings.SymphraSettingsState
import java.io.File
import java.nio.charset.StandardCharsets

private val LOG = Logger.getInstance(SymphraFormatDocumentAction::class.java)

private val BINARY_NAME =
    if (System.getProperty("os.name").lowercase().contains("win")) {
        "symphra-formatter.exe"
    } else {
        "symphra-formatter"
    }

/**
 * Formats the current `.sym` document by piping it through the
 * `symphra-formatter` CLI's stdin/stdout mode (`symphra-formatter -`).
 *
 * This exists as a standalone action rather than wiring into IntelliJ's
 * built-in "Reformat Code" (Ctrl+Alt+L): that action never actually sends
 * `textDocument/formatting` to the language server for this plugin (verified
 * with the LSP server logging every request it receives — none arrived),
 * despite `LspServerDescriptor.lspFormattingSupport` being set and the
 * server correctly advertising `documentFormattingProvider`. Spawning the
 * formatter directly sidesteps that entirely and needs nothing from the
 * experimental `com.intellij.platform.lsp.api` surface beyond what already
 * works (starting the language server itself).
 */
class SymphraFormatDocumentAction : AnAction() {
    override fun getActionUpdateThread(): ActionUpdateThread = ActionUpdateThread.BGT

    override fun update(event: AnActionEvent) {
        val file = event.getData(CommonDataKeys.VIRTUAL_FILE)
        event.presentation.isEnabledAndVisible = file?.fileType == SymphraFileType
    }

    override fun actionPerformed(event: AnActionEvent) {
        val project = event.project ?: return
        val editor = event.getData(CommonDataKeys.EDITOR) ?: return
        val document = editor.document
        val original = document.text

        val formatted = formatWithCli(project, original)
        if (formatted == null || formatted == original) {
            return
        }

        WriteCommandAction.runWriteCommandAction(project, "Format Symphra Document", null, {
            document.setText(formatted)
        })
    }

    private fun formatWithCli(project: Project, source: String): String? {
        val command = resolveFormatterCommand(project)
        val process =
            try {
                ProcessBuilder(command, "-").start()
            } catch (error: java.io.IOException) {
                LOG.warn("could not start symphra-formatter (\"$command\")", error)
                return null
            }

        // Written from a separate thread so a large document can't deadlock
        // against the child filling its stdout pipe before we finish
        // writing all of stdin.
        val writer =
            Thread {
                process.outputStream.use { it.write(source.toByteArray(StandardCharsets.UTF_8)) }
            }
        writer.start()
        val stdout = process.inputStream.bufferedReader(StandardCharsets.UTF_8).readText()
        val stderr = process.errorStream.bufferedReader(StandardCharsets.UTF_8).readText()
        writer.join()
        val exitCode = process.waitFor()

        if (exitCode != 0) {
            LOG.warn("symphra-formatter exited with $exitCode: $stderr")
            return null
        }
        return stdout
    }

    private fun resolveFormatterCommand(project: Project): String {
        val configured = SymphraSettingsState.getInstance().formatterPath
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
