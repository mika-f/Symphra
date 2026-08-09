package dev.symphra.idea

import com.intellij.openapi.fileTypes.LanguageFileType
import com.intellij.openapi.util.IconLoader
import javax.swing.Icon

object SymphraFileType : LanguageFileType(SymphraLanguage) {
    private val ICON: Icon by lazy {
        IconLoader.getIcon("/icons/symphra.svg", SymphraFileType::class.java)
    }

    override fun getName(): String = "Symphra"

    override fun getDescription(): String = "Symphra source file"

    override fun getDefaultExtension(): String = "sym"

    override fun getIcon(): Icon = ICON
}
