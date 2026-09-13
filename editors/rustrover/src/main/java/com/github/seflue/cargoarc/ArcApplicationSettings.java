package com.github.seflue.cargoarc;

import com.intellij.openapi.application.ApplicationManager;
import com.intellij.openapi.components.PersistentStateComponent;
import com.intellij.openapi.components.Service;
import com.intellij.openapi.components.State;
import com.intellij.openapi.components.Storage;
import com.intellij.util.xmlb.XmlSerializerUtil;

/** The cargo-arc binary path, shared by every project. */
@Service(Service.Level.APP)
@State(name = "CargoArc", storages = @Storage("cargo-arc.xml"))
public final class ArcApplicationSettings implements PersistentStateComponent<ArcApplicationSettings.State> {

    private State state = new State();

    public static ArcApplicationSettings getInstance() {
        return ApplicationManager.getApplication().getService(ArcApplicationSettings.class);
    }

    @Override
    public State getState() {
        return state;
    }

    @Override
    public void loadState(State state) {
        XmlSerializerUtil.copyBean(state, this.state);
    }

    public String binary() {
        return state.binary;
    }

    public void setBinary(String binary) {
        state.binary = binary;
    }

    public static final class State {
        public String binary = "cargo-arc";
    }
}
