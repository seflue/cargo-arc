package com.github.seflue.cargoarc;

import com.intellij.openapi.project.Project;
import com.intellij.openapi.vfs.VirtualFile;
import com.intellij.openapi.vfs.newvfs.BulkFileListener;
import com.intellij.openapi.vfs.newvfs.events.VFileContentChangeEvent;
import com.intellij.openapi.vfs.newvfs.events.VFileEvent;

import java.util.List;

/**
 * Sends each local file that a save wrote to the running service. Listens
 * after the write, so the file on disk already holds the saved content when
 * the service reads it.
 */
public final class SaveListener implements BulkFileListener {

    private final Project project;

    public SaveListener(Project project) {
        this.project = project;
    }

    @Override
    public void after(List<? extends VFileEvent> events) {
        ArcService service = project.getServiceIfCreated(ArcService.class);
        if (service == null) {
            return;
        }
        for (VFileEvent event : events) {
            if (!(event instanceof VFileContentChangeEvent change) || !change.isFromSave()) {
                continue;
            }
            VirtualFile file = change.getFile();
            if (file.isInLocalFileSystem()) {
                service.saved(file.toNioPath());
            }
        }
    }
}
