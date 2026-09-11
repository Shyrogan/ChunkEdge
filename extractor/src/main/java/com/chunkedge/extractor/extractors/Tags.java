package com.chunkedge.extractor.extractors;

import com.chunkedge.extractor.Main;
import com.google.gson.JsonArray;
import com.google.gson.JsonElement;
import com.google.gson.JsonObject;
import java.util.Map;
import java.util.TreeMap;
import net.minecraft.core.Registry;
import net.minecraft.core.RegistrySynchronization;
import net.minecraft.server.MinecraftServer;

public class Tags implements Main.Extractor {

    private final MinecraftServer server;

    public Tags(MinecraftServer server) {
        this.server = server;
    }

    @Override
    public String fileName() {
        return "tags.json";
    }

    @Override
    public JsonElement extract() {
        var tagsJson = new JsonObject();

        var registryTags = new TreeMap<String, Map<String, JsonArray>>();
        server
            .registryAccess()
            .registries()
            .filter(entry ->
                RegistrySynchronization.isNetworkable(entry.key())
            )
            .forEach(entry -> {
                var serialized = serializeTags(entry.value());
                if (!serialized.isEmpty()) {
                    registryTags.put(
                        entry.key().identifier().toString(),
                        serialized
                    );
                }
            });

        for (var registry : registryTags.entrySet()) {
            var tagGroupTagsJson = new JsonObject();

            for (var tag : registry.getValue().entrySet()) {
                tagGroupTagsJson.add(tag.getKey(), tag.getValue());
            }

            tagsJson.add(registry.getKey(), tagGroupTagsJson);
        }

        return tagsJson;
    }

    private static <T> Map<String, JsonArray> serializeTags(
        Registry<T> registry
    ) {
        TreeMap<String, JsonArray> map = new TreeMap<>();
        registry
            .getTags()
            .forEach(tag -> {
                var holders = tag.stream().toList();
                JsonArray intList = new JsonArray(holders.size());
                for (var holder : holders) {
                    if (!holder.isBound()) {
                        throw new IllegalStateException(
                            "Can't serialize unregistered value " + holder
                        );
                    }
                    intList.add(registry.getId(holder.value()));
                }
                map.put(tag.key().location().toString(), intList);
            });
        return map;
    }
}
