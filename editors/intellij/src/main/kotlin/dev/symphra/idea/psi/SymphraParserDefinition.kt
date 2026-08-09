package dev.symphra.idea.psi

import com.intellij.extapi.psi.ASTWrapperPsiElement
import com.intellij.lang.ASTNode
import com.intellij.lang.ParserDefinition
import com.intellij.lang.PsiParser
import com.intellij.lexer.Lexer
import com.intellij.openapi.project.Project
import com.intellij.psi.FileViewProvider
import com.intellij.psi.PsiElement
import com.intellij.psi.PsiFile
import com.intellij.psi.TokenType
import com.intellij.psi.tree.IFileElementType
import com.intellij.psi.tree.TokenSet
import dev.symphra.idea.SymphraLanguage
import dev.symphra.idea.lexer.SymphraLexer
import dev.symphra.idea.lexer.SymphraTokenTypes

/**
 * A deliberately trivial PSI layer: one flat file node wrapping the raw
 * [SymphraLexer] token stream, with no internal grammar structure.
 *
 * This exists only so the platform recognizes `.sym` files as PSI files of
 * [SymphraLanguage] rather than generic plain text, which several IDE
 * features key off of — most importantly for this plugin, the built-in LSP
 * "Reformat Code" bridge never sends `textDocument/formatting` at all for a
 * language with no registered parser. Full syntax structure is intentionally
 * not modeled here (see [SymphraLexer]'s doc comment for why); semantic help
 * comes entirely from the symphra-lsp language server.
 */
class SymphraParserDefinition : ParserDefinition {
    override fun createLexer(project: Project): Lexer = SymphraLexer()

    override fun createParser(project: Project): PsiParser =
        PsiParser { root, builder ->
            val marker = builder.mark()
            while (!builder.eof()) {
                builder.advanceLexer()
            }
            marker.done(root)
            builder.treeBuilt
        }

    override fun getFileNodeType(): IFileElementType = FILE

    override fun getWhitespaceTokens(): TokenSet = TokenSet.create(TokenType.WHITE_SPACE)

    override fun getCommentTokens(): TokenSet = TokenSet.create(SymphraTokenTypes.COMMENT)

    override fun getStringLiteralElements(): TokenSet = TokenSet.create(SymphraTokenTypes.STRING)

    override fun createElement(node: ASTNode): PsiElement = ASTWrapperPsiElement(node)

    override fun createFile(viewProvider: FileViewProvider): PsiFile = SymphraFile(viewProvider)

    companion object {
        val FILE = IFileElementType(SymphraLanguage)
    }
}
