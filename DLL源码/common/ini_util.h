#ifndef VIB_INI_UTIL_H
#define VIB_INI_UTIL_H

#include <windows.h>
#include "vib_protocol.h"

/* 读取 INI: 返回 1=成功(节/键存在) */
int  ini_read_int(const char *path, const char *section, const char *key, int def);
void ini_write_int(const char *path, const char *section, const char *key, int val);

/* 读取/写入完整 VibConfig */
int  ini_load_config(const char *path, VibConfig *cfg);
void ini_save_config(const char *path, const VibConfig *cfg);

/* 预设管理: 预设 = 独立 ini 文件(包含 [PadMap]+[Vibration]) */
int  preset_save(const char *presetPath, const VibConfig *cfg);
int  preset_load(const char *presetPath, VibConfig *cfg);
int  preset_delete(const char *presetPath);
/* 列举某目录下 *.ini 预设文件, 返回数量(<=max) */
int  preset_list(const char *dir, char names[][MAX_PATH], int max);

/* 查找配置文件: 1) 指定目录 2) 目录下 us_extend_dll\ 子目录, 返回找到的完整路径 */
int  find_config_path(const char *selfDir, char *out, int outLen);

#endif
