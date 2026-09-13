package com.github.seflue.cargoarc;

import com.intellij.openapi.components.PersistentStateComponent;
import com.intellij.openapi.components.Service;
import com.intellij.openapi.components.State;
import com.intellij.openapi.components.Storage;
import com.intellij.openapi.project.Project;
import com.intellij.util.xmlb.XmlSerializerUtil;

import java.util.ArrayList;
import java.util.List;

/**
 * The per-project cargo-arc launch options: the manifest to analyze, the
 * features to activate, and whether to include tests and external crates.
 */
@Service(Service.Level.PROJECT)
@State(name = "CargoArc", storages = @Storage("cargo-arc.xml"))
public final class ArcProjectSettings implements PersistentStateComponent<ArcProjectSettings.State> {

    private State state = new State();

    public static ArcProjectSettings getInstance(Project project) {
        return project.getService(ArcProjectSettings.class);
    }

    @Override
    public State getState() {
        return state;
    }

    @Override
    public void loadState(State state) {
        XmlSerializerUtil.copyBean(state, this.state);
    }

    public String manifestPath() {
        return state.manifestPath;
    }

    public void setManifestPath(String manifestPath) {
        state.manifestPath = manifestPath;
    }

    public String features() {
        return state.features;
    }

    public void setFeatures(String features) {
        state.features = features;
    }

    public List<String> featureList() {
        return parseFeatures(state.features);
    }

    public boolean includeTests() {
        return state.includeTests;
    }

    public void setIncludeTests(boolean includeTests) {
        state.includeTests = includeTests;
    }

    public boolean externals() {
        return state.externals;
    }

    public void setExternals(boolean externals) {
        state.externals = externals;
    }

    /** Splits a comma-separated feature list, dropping blanks left by stray commas or spaces. */
    static List<String> parseFeatures(String raw) {
        List<String> features = new ArrayList<>();
        for (String feature : raw.split(",")) {
            String trimmed = feature.trim();
            if (!trimmed.isEmpty()) {
                features.add(trimmed);
            }
        }
        return features;
    }

    public static final class State {
        public String manifestPath = "";
        public String features = "";
        public boolean includeTests = false;
        public boolean externals = false;
    }
}
