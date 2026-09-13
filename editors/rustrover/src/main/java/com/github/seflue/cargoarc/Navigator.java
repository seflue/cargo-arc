package com.github.seflue.cargoarc;

import java.nio.file.Path;

/** Puts the cursor on a location the service reported. */
@FunctionalInterface
public interface Navigator {

    void jump(Path file, int line);
}
