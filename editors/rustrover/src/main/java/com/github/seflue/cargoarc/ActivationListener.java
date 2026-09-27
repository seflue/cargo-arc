package com.github.seflue.cargoarc;

import com.intellij.openapi.application.ApplicationActivationListener;
import com.intellij.openapi.fileEditor.FileEditorManager;
import com.intellij.openapi.project.Project;
import com.intellij.openapi.wm.IdeFrame;

/**
 * Sends the selected editor's file when RustRover comes to the foreground,
 * as the Neovim plugin does on {@code FocusGained}. The file is the one
 * selected in the project of the window that came to the foreground.
 */
public final class ActivationListener implements ApplicationActivationListener {

    @Override
    public void applicationActivated(IdeFrame ideFrame) {
        Project project = ideFrame.getProject();
        if (project == null || project.isDisposed()) {
            return;
        }
        EditorSelectionListener.sendFocus(project, FileEditorManager.getInstance(project).getSelectedEditor());
    }
}
