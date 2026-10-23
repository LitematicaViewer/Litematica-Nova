package net.litematicaba.mc26oracle;

import java.io.IOException;
import java.nio.file.AtomicMoveNotSupportedException;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.StandardCopyOption;
import java.util.Comparator;
import java.util.Map;

public final class HeadlessOracleMain {
    private HeadlessOracleMain() {}

    public static void main(String[] args) throws Exception {
        if ((args.length != 6 && args.length != 8) || !"--output".equals(args[0])
                || !"--minecraft".equals(args[2]) || !"--client-sha256".equals(args[4])
                || args[1].isBlank() || args[3].isBlank() || !args[5].matches("[0-9a-fA-F]{64}")
                || (args.length == 8 && (!args[6].equals("--scope") || !(args[7].equals("render") || args[7].equals("full"))))) {
            throw new IllegalArgumentException("HeadlessOracleMain --output <new-directory> --minecraft <actual-version> --client-sha256 <actual-jar-sha256> [--scope render|full]");
        }
        Path requestedOutput = Path.of(args[1]);
        Path output = requestedOutput.toAbsolutePath().normalize();
        if (Files.exists(output)) {
            throw new IllegalArgumentException("output already exists: " + output);
        }
        Path parent = output.getParent();
        if (parent == null) {
            throw new IllegalArgumentException("output has no parent: " + output);
        }
        Files.createDirectories(parent);
        Path staging = Files.createTempDirectory(parent, "." + output.getFileName() + ".tmp-");
        boolean installed = false;
        Throwable primaryFailure = null;
        try {
            long started = System.nanoTime();
            MinecraftRuntimeOracle.ExportResult result =
                    new MinecraftRuntimeOracle(args[3], args[5].toLowerCase(java.util.Locale.ROOT), args.length == 6 || !"render".equals(args[7])).export(staging);
            long elapsedMillis =
                    java.time.Duration.ofNanos(System.nanoTime() - started).toMillis();
            install(staging, output);
            installed = true;
            System.out.println(
                    CanonicalJsonl.json(
                            Map.ofEntries(
                                    Map.entry("artifact_count", result.artifacts().size()),
                                    Map.entry("export_elapsed_ms", elapsedMillis),
                                    Map.entry("output", output.toString()),
                                    Map.entry(
                                            "semantics_elapsed_ms",
                                            result.semanticsElapsedMillis()),
                                    Map.entry(
                                            "skip_evaluated_comparisons",
                                            result.skipEvaluatedComparisons()),
                                    Map.entry(
                                            "skip_elapsed_ms", result.skipElapsedMillis()),
                                    Map.entry("status", "ready-headless"))));
        } catch (Exception | Error failure) {
            primaryFailure = failure;
            throw failure;
        } finally {
            if (!installed) {
                try {
                    deleteTree(staging);
                } catch (IOException cleanupFailure) {
                    if (primaryFailure != null) {
                        primaryFailure.addSuppressed(cleanupFailure);
                    } else {
                        throw cleanupFailure;
                    }
                }
            }
        }
    }

    private static void install(Path staging, Path output) throws IOException {
        try {
            Files.move(staging, output, StandardCopyOption.ATOMIC_MOVE);
        } catch (AtomicMoveNotSupportedException ignored) {
            Files.move(staging, output);
        }
    }

    static void deleteTree(Path root) throws IOException {
        if (!Files.exists(root)) {
            return;
        }
        try (var paths = Files.walk(root)) {
            IOException[] failure = new IOException[1];
            paths.sorted(Comparator.reverseOrder())
                    .forEach(
                            path -> {
                                try {
                                    Files.delete(path);
                                } catch (IOException error) {
                                    if (failure[0] == null) {
                                        failure[0] = error;
                                    } else {
                                        failure[0].addSuppressed(error);
                                    }
                                }
                            });
            if (failure[0] != null) {
                throw failure[0];
            }
        }
    }
}
