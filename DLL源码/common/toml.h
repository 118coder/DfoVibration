#ifndef VIB_TOML_H
#define VIB_TOML_H

#include <windows.h>
#include "vib_protocol.h"

/* TOML 子集解析(支持 Sorahk 配置格式):
 *   注释 # ;   表 [name]   数组表 [[name]] / [[a]] 嵌套 [[a.b]]
 *   键值: key="str"  key=[ "a","b" ]  key=123  key=true/false
 * 只做线性扫描, 不构建完整树: 通过回调按"路径"匹配行。 */

#define TOML_MAX_PATH 256

/* 按路径读取一行值: path 如 "presets[0].mappings[1].target_keys"
 * 返回: 0=无, 1=字符串值(填 str), 2=整数, 3=布尔, 4=字符串数组(逐项回调) */
typedef void (*TomlStrArrayCb)(const char *item, void *user);

int  toml_get_str(const char *text, const char *path, const char *key, char *out, int outLen);
int  toml_get_int(const char *text, const char *path, const char *key, int def);
int  toml_get_bool(const char *text, const char *path, const char *key, int def);
/* 遍历字符串数组元素 */
void toml_foreach_str(const char *text, const char *path, const char *key,
                      TomlStrArrayCb cb, void *user);

/* 通用: 扫描文本, 返回数组表实例计数(如 [[presets]] 的个数) */
int  toml_array_count(const char *text, const char *arrayName);

/* 写出(简化, 供保存): 生成 [[presets]] 风格文本 */
typedef struct {
    const char *name;
    const VibMapEntry *maps;
    int count;
} TomlPresetOut;

void toml_write(const char *path,
                const VibSettings *st,
                const TomlPresetOut *presets, int presetCount);

#endif
