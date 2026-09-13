package com.github.seflue.cargoarc;

import com.intellij.openapi.options.Configurable;
import com.intellij.openapi.project.Project;
import com.intellij.util.ui.FormBuilder;
import org.jetbrains.annotations.Nullable;

import javax.swing.JCheckBox;
import javax.swing.JComponent;
import javax.swing.JPanel;
import javax.swing.JTextField;

/** The Settings > Tools > cargo-arc page: the binary path and this project's launch options. */
public final class ArcConfigurable implements Configurable {

    private final ArcApplicationSettings applicationSettings = ArcApplicationSettings.getInstance();
    private final ArcProjectSettings projectSettings;

    private JTextField binaryField;
    private JTextField manifestPathField;
    private JTextField featuresField;
    private JCheckBox includeTestsCheckBox;
    private JCheckBox externalsCheckBox;

    public ArcConfigurable(Project project) {
        this.projectSettings = ArcProjectSettings.getInstance(project);
    }

    @Override
    public String getDisplayName() {
        return "cargo-arc";
    }

    @Override
    public @Nullable JComponent createComponent() {
        binaryField = new JTextField();
        manifestPathField = new JTextField();
        featuresField = new JTextField();
        includeTestsCheckBox = new JCheckBox();
        externalsCheckBox = new JCheckBox();

        return FormBuilder.createFormBuilder()
            .addLabeledComponent("binary", binaryField)
            .addLabeledComponent("manifest_path", manifestPathField)
            .addLabeledComponent("features", featuresField)
            .addLabeledComponent("include_tests", includeTestsCheckBox)
            .addLabeledComponent("externals", externalsCheckBox)
            .addComponentFillVertically(new JPanel(), 0)
            .getPanel();
    }

    @Override
    public boolean isModified() {
        return !binaryField.getText().equals(applicationSettings.binary())
            || !manifestPathField.getText().equals(projectSettings.manifestPath())
            || !featuresField.getText().equals(projectSettings.features())
            || includeTestsCheckBox.isSelected() != projectSettings.includeTests()
            || externalsCheckBox.isSelected() != projectSettings.externals();
    }

    @Override
    public void apply() {
        applicationSettings.setBinary(binaryField.getText());
        projectSettings.setManifestPath(manifestPathField.getText());
        projectSettings.setFeatures(featuresField.getText());
        projectSettings.setIncludeTests(includeTestsCheckBox.isSelected());
        projectSettings.setExternals(externalsCheckBox.isSelected());
    }

    @Override
    public void reset() {
        binaryField.setText(applicationSettings.binary());
        manifestPathField.setText(projectSettings.manifestPath());
        featuresField.setText(projectSettings.features());
        includeTestsCheckBox.setSelected(projectSettings.includeTests());
        externalsCheckBox.setSelected(projectSettings.externals());
    }
}
