package dev.symphra.idea.settings

import com.intellij.openapi.components.PersistentStateComponent
import com.intellij.openapi.components.Service
import com.intellij.openapi.components.State
import com.intellij.openapi.components.Storage
import com.intellij.openapi.components.service

@Service(Service.Level.APP)
@State(name = "SymphraSettings", storages = [Storage("symphra.xml")])
class SymphraSettingsState : PersistentStateComponent<SymphraSettingsState.State> {
    data class State(var serverPath: String = "", var formatterPath: String = "")

    private var myState = State()

    override fun getState(): State = myState

    override fun loadState(state: State) {
        myState = state
    }

    var serverPath: String
        get() = myState.serverPath
        set(value) {
            myState.serverPath = value
        }

    var formatterPath: String
        get() = myState.formatterPath
        set(value) {
            myState.formatterPath = value
        }

    companion object {
        fun getInstance(): SymphraSettingsState = service()
    }
}
