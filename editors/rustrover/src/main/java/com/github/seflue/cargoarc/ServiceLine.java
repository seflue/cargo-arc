package com.github.seflue.cargoarc;

import java.nio.file.Path;
import java.util.Optional;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

/**
 * One line of the service's stdout, parsed into what it announces.
 */
public sealed interface ServiceLine {

    Pattern READY = Pattern.compile("^arc ready (\\S+) (\\d+)$");
    Pattern JUMP = Pattern.compile("^arc jump (\\d+) (.+)$");

    /** The service is listening on {@code port}, running version {@code version}. */
    record Ready(String version, int port) implements ServiceLine {}

    /** The service asks to put the cursor on {@code line} of {@code file}. */
    record Jump(int line, Path file) implements ServiceLine {}

    /**
     * Parses one line of stdout. Empty for anything that is neither an
     * {@code arc ready} nor an {@code arc jump} line.
     */
    static Optional<ServiceLine> parse(String line) {
        Matcher ready = READY.matcher(line);
        if (ready.matches()) {
            return Optional.of(new Ready(ready.group(1), Integer.parseInt(ready.group(2))));
        }
        Matcher jump = JUMP.matcher(line);
        if (jump.matches()) {
            return Optional.of(new Jump(Integer.parseInt(jump.group(1)), Path.of(jump.group(2))));
        }
        return Optional.empty();
    }
}
