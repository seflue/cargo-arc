package com.github.seflue.cargoarc;

import com.intellij.openapi.fileEditor.FileEditor;
import com.intellij.openapi.fileEditor.FileEditorManagerEvent;
import com.intellij.openapi.fileEditor.FileEditorManagerListener;
import com.intellij.openapi.fileEditor.TextEditor;
import com.intellij.openapi.project.Project;
import com.intellij.openapi.vfs.VirtualFile;

/** Sends the file of the newly selected editor to a running service. */
public final class EditorSelectionListener implements FileEditorManagerListener {

    private final Project project;

    public EditorSelectionListener(Project project) {
        this.project = project;
    }

    @Override
    public void selectionChanged(FileEditorManagerEvent event) {
        sendFocus(project, event.getNewEditor());
    }

    /**
     * Sends the caret line of {@code editor}'s file, or line 1 for an editor
     * without a caret. Sends nothing for an editor that shows no local file,
     * or while the project has no service.
     */
    static void sendFocus(Project project, FileEditor editor) {
        if (editor == null) {
            return;
        }
        VirtualFile file = editor.getFile();
        if (file == null || !file.isInLocalFileSystem()) {
            return;
        }
        ArcService service = project.getServiceIfCreated(ArcService.class);
        if (service == null) {
            return;
        }
        int line = editor instanceof TextEditor text
            ? text.getEditor().getCaretModel().getLogicalPosition().line + 1
            : 1;
        service.focus(file.toNioPath(), line);
    }
}
