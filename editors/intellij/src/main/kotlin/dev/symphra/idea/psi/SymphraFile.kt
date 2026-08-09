package dev.symphra.idea.psi

import com.intellij.extapi.psi.PsiFileBase
import com.intellij.openapi.fileTypes.FileType
import com.intellij.psi.FileViewProvider
import dev.symphra.idea.SymphraFileType
import dev.symphra.idea.SymphraLanguage

class SymphraFile(viewProvider: FileViewProvider) : PsiFileBase(viewProvider, SymphraLanguage) {
    override fun getFileType(): FileType = SymphraFileType

    override fun toString(): String = "Symphra File"
}
