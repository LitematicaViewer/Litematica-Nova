package net.litematicaba.mc26oracle;

import java.lang.invoke.MethodHandle;
import java.lang.invoke.MethodHandles;
import java.lang.invoke.MethodType;
import java.lang.reflect.Field;
import java.lang.reflect.InvocationTargetException;
import java.lang.reflect.Method;
import java.time.Duration;
import java.util.ArrayList;
import java.util.LinkedHashMap;
import java.util.List;
import java.util.Locale;
import java.util.Map;
import java.util.Set;
import java.util.TreeSet;
import java.util.concurrent.Callable;
import java.util.concurrent.ExecutionException;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;
import java.util.concurrent.Future;
import java.util.concurrent.ThreadFactory;
import java.util.concurrent.atomic.AtomicInteger;
import java.util.concurrent.atomic.AtomicLong;

final class SkipRenderingOracle {
    static final String BUDGET_SECONDS_PROPERTY = "lba.mc26.oracle.skipBudgetSeconds";
    static final String THREADS_PROPERTY = "lba.mc26.oracle.skipThreads";
    private static final List<String> PROFILES = List.of("cutout", "opaque");

    private final Class<?> blockBehaviour =
            ReflectionAccess.type("net.minecraft.world.level.block.state.BlockBehaviour");
    private final Class<?> blockState =
            ReflectionAccess.type("net.minecraft.world.level.block.state.BlockState");
    private final Class<?> blockStateBase =
            ReflectionAccess.type(
                    "net.minecraft.world.level.block.state.BlockBehaviour$BlockStateBase");
    private final Class<?> direction = ReflectionAccess.type("net.minecraft.core.Direction");
    private final Class<?> leavesBlock =
            ReflectionAccess.type("net.minecraft.world.level.block.LeavesBlock");
    private final MethodHandle skipRendering;
    private final Method setCutoutLeaves;
    private final Field cutoutLeaves;
    private final Object[] directions;

    SkipRenderingOracle() {
        try {
            Method skip = ReflectionAccess.method(blockStateBase, "skipRendering", blockState, direction);
            skipRendering =
                    MethodHandles.lookup()
                            .unreflect(skip)
                            .asType(
                                    MethodType.methodType(
                                            boolean.class,
                                            Object.class,
                                            Object.class,
                                            Object.class));
            if (ReflectionAccess.MAPPING.hasMethod(leavesBlock, "setCutoutLeaves", 1)) {
                setCutoutLeaves = ReflectionAccess.method(leavesBlock, "setCutoutLeaves", boolean.class);
                cutoutLeaves = ReflectionAccess.fieldHandle(leavesBlock, "cutoutLeaves");
                cutoutLeaves.setAccessible(true);
            } else { setCutoutLeaves = null; cutoutLeaves = null; }
            directions = direction.getEnumConstants();
            if (directions == null || directions.length != 6) {
                throw new IllegalStateException(
                        "expected six Minecraft directions, got "
                                + (directions == null ? "null" : directions.length));
            }
        } catch (ReflectiveOperationException error) {
            throw new IllegalStateException("cannot initialize skipRendering oracle", error);
        }
    }

    ExportData export(List<BlockStateSemanticsOracle.CatalogState> catalog) {
        long started = System.nanoTime();
        long deadline = deadlineNanos();
        int threads = configuredThreads();
        List<SourceState> commonSources = new ArrayList<>();
        List<SourceState> leafSources = new ArrayList<>();
        Set<String> sourceBlocks = new TreeSet<>();
        Set<String> overrideOwners = new TreeSet<>();

        for (BlockStateSemanticsOracle.CatalogState state : catalog) {
            Method override = effectiveSkipRendering(state.block().getClass());
            if (override.getDeclaringClass() == blockBehaviour) {
                continue;
            }
            SourceState source =
                    new SourceState(state, override.getDeclaringClass().getName());
            if (leavesBlock.isInstance(state.block())) {
                leafSources.add(source);
            } else {
                commonSources.add(source);
            }
            sourceBlocks.add(state.blockId());
            overrideOwners.add(source.overrideOwner());
        }

        long expectedComparisons =
                Math.multiplyExact(
                        Math.addExact(commonSources.size(), Math.multiplyExact(2, leafSources.size())),
                        Math.multiplyExact((long) catalog.size(), directions.length));
        AtomicLong evaluatedComparisons = new AtomicLong();
        List<Row> rows = new ArrayList<>();
        boolean originalCutout = getCutoutLeaves();
        ExecutorService executor = Executors.newFixedThreadPool(threads, threadFactory());
        Throwable primaryFailure = null;
        try {
            List<Row> common =
                    evaluateBatch(
                            executor,
                            "cutout",
                            commonSources,
                            catalog,
                            deadline,
                            evaluatedComparisons);
            rows.addAll(common);
            common.stream().map(row -> row.withProfile("opaque")).forEach(rows::add);

            setCutoutLeaves(true);
            rows.addAll(
                    evaluateBatch(
                            executor,
                            "cutout",
                            leafSources,
                            catalog,
                            deadline,
                            evaluatedComparisons));
            setCutoutLeaves(false);
            rows.addAll(
                    evaluateBatch(
                            executor,
                            "opaque",
                            leafSources,
                            catalog,
                            deadline,
                            evaluatedComparisons));
        } catch (RuntimeException | Error failure) {
            primaryFailure = failure;
            throw failure;
        } finally {
            executor.shutdownNow();
            try {
                setCutoutLeaves(originalCutout);
            } catch (RuntimeException restoreFailure) {
                if (primaryFailure != null) {
                    primaryFailure.addSuppressed(restoreFailure);
                } else {
                    throw restoreFailure;
                }
            }
        }

        if (evaluatedComparisons.get() != expectedComparisons) {
            throw new IllegalStateException(
                    "skipRendering comparison count mismatch: expected="
                            + expectedComparisons
                            + " actual="
                            + evaluatedComparisons.get());
        }

        List<Map<String, ?>> records = new ArrayList<>(rows.size());
        rows.stream().map(Row::asRecord).forEach(records::add);
        Map<String, Object> manifest = new LinkedHashMap<>();
        manifest.put("evaluated_comparisons", evaluatedComparisons.get());
        manifest.put("override_owners", List.copyOf(overrideOwners));
        manifest.put("profiles", PROFILES);
        manifest.put("leaves_profile_source", setCutoutLeaves == null
                ? "target_runtime_has_no_mutable_cutoutLeaves:both_profiles_use_actual_skipRendering"
                : "LeavesBlock.setCutoutLeaves(boolean)");
        manifest.put("source_block_ids", List.copyOf(sourceBlocks));
        manifest.put("source_state_count", commonSources.size() + leafSources.size());
        manifest.put("sparse_positive_row_count", records.size());
        return new ExportData(
                records,
                manifest,
                evaluatedComparisons.get(),
                Duration.ofNanos(System.nanoTime() - started).toMillis());
    }

    private List<Row> evaluateBatch(
            ExecutorService executor,
            String profile,
            List<SourceState> sources,
            List<BlockStateSemanticsOracle.CatalogState> catalog,
            long deadline,
            AtomicLong evaluatedComparisons) {
        List<Callable<List<Row>>> tasks = new ArrayList<>(sources.size());
        for (SourceState source : sources) {
            tasks.add(
                    () ->
                            evaluateSource(
                                    profile,
                                    source,
                                    catalog,
                                    deadline,
                                    evaluatedComparisons));
        }
        try {
            List<Future<List<Row>>> futures = executor.invokeAll(tasks);
            List<Row> rows = new ArrayList<>();
            for (Future<List<Row>> future : futures) {
                rows.addAll(future.get());
            }
            return rows;
        } catch (InterruptedException error) {
            Thread.currentThread().interrupt();
            throw new IllegalStateException(
                    "skipRendering export interrupted after "
                            + evaluatedComparisons.get()
                            + " comparisons",
                    error);
        } catch (ExecutionException error) {
            Throwable cause = error.getCause();
            if (cause instanceof RuntimeException runtimeException) {
                throw runtimeException;
            }
            if (cause instanceof Error invocationError) {
                throw invocationError;
            }
            throw new IllegalStateException("skipRendering worker failed", cause);
        }
    }

    private List<Row> evaluateSource(
            String profile,
            SourceState source,
            List<BlockStateSemanticsOracle.CatalogState> catalog,
            long deadline,
            AtomicLong evaluatedComparisons) {
        List<Row> rows = new ArrayList<>(directions.length);
        for (Object directionValue : directions) {
            RangeAccumulator ranges = new RangeAccumulator();
            for (int neighborIndex = 0; neighborIndex < catalog.size(); neighborIndex++) {
                if ((neighborIndex & 0x3FF) == 0) {
                    ensureBudget(deadline, evaluatedComparisons.get());
                }
                boolean skip =
                        invokeSkip(
                                source.state().state(),
                                catalog.get(neighborIndex).state(),
                                directionValue);
                evaluatedComparisons.incrementAndGet();
                ranges.accept(neighborIndex, skip);
            }
            List<Range> result = ranges.finish(catalog.size());
            if (!result.isEmpty()) {
                rows.add(
                        new Row(
                                directionName(directionValue),
                                result,
                                source.overrideOwner(),
                                profile,
                                source.state().catalogStateIndex(),
                                source.state().stateId()));
            }
        }
        return rows;
    }

    private Method effectiveSkipRendering(Class<?> concreteBlock) {
        Class<?> current = concreteBlock;
        while (current != null) {
            try {
                Method method =
                        current.getDeclaredMethod(
                                ReflectionAccess.method(blockBehaviour, "skipRendering", blockState, blockState, direction).getName(),
                                blockState, blockState, direction);
                method.setAccessible(true);
                return method;
            } catch (NoSuchMethodException ignored) {
                if (current == blockBehaviour) {
                    break;
                }
                current = current.getSuperclass();
            }
        }
        throw new IllegalStateException(
                "cannot resolve effective skipRendering for " + concreteBlock.getName());
    }

    private boolean invokeSkip(Object source, Object neighbor, Object directionValue) {
        try {
            return (boolean) skipRendering.invokeExact(source, neighbor, directionValue);
        } catch (RuntimeException | Error error) {
            throw error;
        } catch (Throwable error) {
            throw new IllegalStateException("skipRendering invocation failed", error);
        }
    }

    private boolean getCutoutLeaves() {
        if (cutoutLeaves == null) return true;
        try {
            return cutoutLeaves.getBoolean(null);
        } catch (IllegalAccessException error) {
            throw new IllegalStateException("cannot read LeavesBlock.cutoutLeaves", error);
        }
    }

    private void setCutoutLeaves(boolean cutout) {
        if (setCutoutLeaves == null) return;
        try {
            setCutoutLeaves.invoke(null, cutout);
        } catch (InvocationTargetException error) {
            Throwable cause = error.getCause();
            if (cause instanceof RuntimeException runtimeException) {
                throw runtimeException;
            }
            if (cause instanceof Error invocationError) {
                throw invocationError;
            }
            throw new IllegalStateException("LeavesBlock.setCutoutLeaves failed", cause);
        } catch (IllegalAccessException error) {
            throw new IllegalStateException("cannot set LeavesBlock.cutoutLeaves", error);
        }
    }

    private static long deadlineNanos() {
        String configured = System.getProperty(BUDGET_SECONDS_PROPERTY, "0");
        long seconds;
        try {
            seconds = Long.parseLong(configured);
        } catch (NumberFormatException error) {
            throw new IllegalArgumentException(
                    BUDGET_SECONDS_PROPERTY + " must be a non-negative integer", error);
        }
        if (seconds < 0) {
            throw new IllegalArgumentException(
                    BUDGET_SECONDS_PROPERTY + " must be a non-negative integer");
        }
        if (seconds == 0) {
            return Long.MAX_VALUE;
        }
        long budget;
        try {
            budget = Math.multiplyExact(seconds, 1_000_000_000L);
            return Math.addExact(System.nanoTime(), budget);
        } catch (ArithmeticException error) {
            return Long.MAX_VALUE;
        }
    }

    private static int configuredThreads() {
        int defaultThreads = Math.min(8, Math.max(1, Runtime.getRuntime().availableProcessors()));
        String configured = System.getProperty(THREADS_PROPERTY);
        if (configured == null) {
            return defaultThreads;
        }
        int threads;
        try {
            threads = Integer.parseInt(configured);
        } catch (NumberFormatException error) {
            throw new IllegalArgumentException(THREADS_PROPERTY + " must be an integer", error);
        }
        if (threads < 1 || threads > 8) {
            throw new IllegalArgumentException(THREADS_PROPERTY + " must be between 1 and 8");
        }
        return threads;
    }

    private static void ensureBudget(long deadline, long comparisons) {
        if (Thread.currentThread().isInterrupted()) {
            throw new IllegalStateException(
                    "skipRendering export interrupted after " + comparisons + " comparisons");
        }
        if (deadline != Long.MAX_VALUE && System.nanoTime() - deadline >= 0) {
            throw new IllegalStateException(
                    "skipRendering export exceeded "
                            + BUDGET_SECONDS_PROPERTY
                            + " after "
                            + comparisons
                            + " comparisons; no partial catalog was installed");
        }
    }

    private static ThreadFactory threadFactory() {
        AtomicInteger sequence = new AtomicInteger();
        return runnable -> {
            Thread thread =
                    new Thread(runnable, "mc26-skip-oracle-" + sequence.incrementAndGet());
            thread.setDaemon(true);
            return thread;
        };
    }

    private static String directionName(Object directionValue) {
        return ((Enum<?>) directionValue).name().toLowerCase(Locale.ROOT);
    }

    record ExportData(
            List<Map<String, ?>> records,
            Map<String, Object> manifest,
            long evaluatedComparisons,
            long elapsedMillis) {}

    record Range(int start, int endExclusive) {
        Range {
            if (start < 0 || endExclusive <= start) {
                throw new IllegalArgumentException(
                        "invalid catalog index range [" + start + "," + endExclusive + ")");
            }
        }

        Map<String, Object> asRecord() {
            return Map.of("end_exclusive", endExclusive, "start", start);
        }
    }

    static final class RangeAccumulator {
        private final List<Range> ranges = new ArrayList<>();
        private int openStart = -1;
        private int nextIndex;

        void accept(int index, boolean included) {
            if (index != nextIndex) {
                throw new IllegalArgumentException(
                        "range indexes must be contiguous: expected="
                                + nextIndex
                                + " actual="
                                + index);
            }
            if (included && openStart < 0) {
                openStart = index;
            } else if (!included && openStart >= 0) {
                ranges.add(new Range(openStart, index));
                openStart = -1;
            }
            nextIndex++;
        }

        List<Range> finish(int size) {
            if (size != nextIndex) {
                throw new IllegalArgumentException(
                        "range size mismatch: expected=" + nextIndex + " actual=" + size);
            }
            if (openStart >= 0) {
                ranges.add(new Range(openStart, size));
                openStart = -1;
            }
            return List.copyOf(ranges);
        }
    }

    private record SourceState(
            BlockStateSemanticsOracle.CatalogState state, String overrideOwner) {}

    private record Row(
            String direction,
            List<Range> neighborRanges,
            String overrideOwner,
            String profile,
            int sourceCatalogStateIndex,
            String sourceStateId) {
        Row withProfile(String replacement) {
            return new Row(
                    direction,
                    neighborRanges,
                    overrideOwner,
                    replacement,
                    sourceCatalogStateIndex,
                    sourceStateId);
        }

        Map<String, Object> asRecord() {
            Map<String, Object> record = new LinkedHashMap<>();
            record.put("direction", direction);
            record.put("neighbor_catalog_index_ranges", neighborRanges.stream().map(Range::asRecord).toList());
            record.put("override_owner", overrideOwner);
            record.put("profile", profile);
            record.put("source_catalog_state_index", sourceCatalogStateIndex);
            record.put("source_state_id", sourceStateId);
            return record;
        }
    }
}
