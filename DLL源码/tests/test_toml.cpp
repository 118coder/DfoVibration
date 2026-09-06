/* TOML 解析调试工具 */
#include <stdio.h>
#include <string.h>
#include "../common/toml.h"

static void cb(const char *item, void *user)
{
    printf("    item: %s\n", item);
}

int main(int argc, char **argv)
{
    const char *path = argv[1];
    char buf[256];
    int np, i;

    np = toml_array_count(path, "presets");
    printf("presets count: %d\n", np);
    if (np > 0) {
        char p[64];
        _snprintf(p, sizeof(p), "presets[0]");
        if (toml_get_str(path, p, "name", buf, sizeof(buf)))
            printf("name: %s\n", buf);
        else
            printf("name: FAILED\n");
        {
            char mp[64];
            _snprintf(mp, sizeof(mp), "presets[0].mappings");
            int nm = toml_array_count(path, mp);
            printf("mappings count: %d\n", nm);
            for (i = 0; i < nm && i < 3; i++) {
                char sub[64];
                _snprintf(sub, sizeof(sub), "presets[0].mappings[%d]", i);
                if (toml_get_str(path, sub, "trigger_key", buf, sizeof(buf)))
                    printf("  [%d] trigger: %s\n", i, buf);
                else
                    printf("  [%d] trigger: FAILED\n", i);
                printf("  [%d] target_keys:\n", i);
                toml_foreach_str(path, sub, "target_keys", cb, NULL);
                printf("  [%d] turbo: %d interval: %d note: ", i,
                       toml_get_bool(path, sub, "turbo_enabled", 0),
                       toml_get_int(path, sub, "interval", 0));
                if (toml_get_str(path, sub, "note", buf, sizeof(buf)))
                    printf("%s\n", buf);
                else
                    printf("(none)\n");
            }
        }
    }
    return 0;
}
