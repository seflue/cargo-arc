package com.github.seflue.cargoarc;

import java.util.ArrayList;
import java.util.List;

/**
 * The command line that starts the service. Shared flags sit on {@code arc},
 * before the {@code ui} subcommand, mirroring the Neovim plugin's
 * {@code M.argv}. No {@code --port}: the service always picks a free one.
 */
public record LaunchOptions(
    String binary,
    String manifestPath,
    List<String> features,
    boolean includeTests,
    boolean externals
) {

    public List<String> argv() {
        List<String> argv = new ArrayList<>();
        argv.add(binary);
        argv.add("arc");
        if (manifestPath != null) {
            argv.add("--manifest-path");
            argv.add(manifestPath);
        }
        if (!features.isEmpty()) {
            argv.add("--features");
            argv.add(String.join(",", features));
        }
        if (includeTests) {
            argv.add("--include-tests");
        }
        if (externals) {
            argv.add("--externals");
        }
        argv.add("ui");
        return argv;
    }
}
