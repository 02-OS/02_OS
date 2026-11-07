#include <assert.h>
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <cairo.h>
#include "script.h"
#include "script-parse.h"
#include "script-execute.h"
#include "script-object.h"
#include "script-lib-image.h"
#include "script-lib-sprite.h"
#include "script-lib-plymouth.h"
#include "script-lib-math.h"
#include "script-lib-string.h"
#include "ply-list.h"

static double number(script_state_t *s, const char *name) {
    return script_obj_hash_get_number(s->global, name);
}
static char *string(script_state_t *s, const char *name) {
    return script_obj_hash_get_string(s->global, name);
}
static sprite_t *sprite_at(script_lib_sprite_data_t *d, int z) {
    for (ply_list_node_t *n = ply_list_get_first_node(d->sprite_list); n; n = ply_list_get_next_node(d->sprite_list,n)) {
        sprite_t *sprite = ply_list_node_get_data(n);
        if (sprite->z == z) return sprite;
    }
    return NULL;
}
static void screenshot(script_lib_sprite_data_t *d, const char *name) {
    ply_pixel_buffer_t *buffer = ply_pixel_buffer_new(d->max_width,d->max_height);
    ply_pixel_buffer_fill_with_hex_color(buffer,NULL,0xf9f9f7ff);
    for (int z=0; z<=10; z++) {
        for (ply_list_node_t *n=ply_list_get_first_node(d->sprite_list); n; n=ply_list_get_next_node(d->sprite_list,n)) {
            sprite_t *sprite=ply_list_node_get_data(n);
            if (sprite->z != z || !sprite->image || sprite->opacity <= 0) continue;
            ply_pixel_buffer_fill_with_buffer_at_opacity(buffer,sprite->image,sprite->x,sprite->y,sprite->opacity);
        }
    }
    cairo_surface_t *surface=cairo_image_surface_create_for_data((unsigned char *)ply_pixel_buffer_get_argb32_data(buffer),CAIRO_FORMAT_ARGB32,d->max_width,d->max_height,d->max_width*4);
    assert(cairo_surface_write_to_png(surface,name) == CAIRO_STATUS_SUCCESS);
    cairo_surface_destroy(surface);
    ply_pixel_buffer_free(buffer);
}

int main(int argc, char **argv) {
    assert(argc == 3);
    const char *dir=argv[1];
    int shutdown=atoi(argv[2]);
    char path[4096];
    snprintf(path,sizeof(path),"%s/02-zero-portal.script",dir);
    script_op_t *op=script_parse_file(path);
    assert(op);
    script_state_t *s=script_state_new(NULL);
    ply_list_t *displays=ply_list_new();
    script_lib_image_setup(s,(char *)dir);
    script_lib_sprite_data_t *sprites=script_lib_sprite_setup(s,displays);
    sprites->max_width=1920;
    sprites->max_height=1080;
    script_lib_plymouth_data_t *plymouth=script_lib_plymouth_setup(s,shutdown ? PLY_BOOT_SPLASH_MODE_SHUTDOWN : PLY_BOOT_SPLASH_MODE_BOOT_UP,30,NULL);
    script_lib_math_setup(s);
    script_lib_string_setup(s);
    script_return_t result=script_execute(s,op);
    assert(result.type != SCRIPT_RETURN_TYPE_FAIL);
    script_obj_unref(result.object);
    assert(sprite_at(sprites,2)->image);
    assert(sprite_at(sprites,1) == NULL);
    assert(number(s,"last_frame") == -1);
    if (shutdown) {
        assert(number(s,"finished") == 1);
        assert(sprite_at(sprites,2)->opacity == 1);
        script_lib_plymouth_on_boot_progress(s,plymouth,42,0.9);
        assert(number(s,"elapsed") == 0);
        puts("PASS shutdown never animates");
        return 0;
    }
    assert(sprite_at(sprites,2)->opacity == 0);
    script_lib_plymouth_on_boot_progress(s,plymouth,10,0.3);
    script_lib_plymouth_on_boot_progress(s,plymouth,10.125,0.4);
    assert(fabs(number(s,"elapsed")-0.125) < 1e-6);
    assert(fabs(sprite_at(sprites,2)->opacity-0.5) < 1e-6);
    for (int i=0; i<200; i++) script_lib_plymouth_on_refresh(s,plymouth);
    assert(fabs(number(s,"elapsed")-0.125) < 1e-6);
    assert(fabs(sprite_at(sprites,2)->opacity-0.5) < 1e-6);
    script_lib_plymouth_on_boot_progress(s,plymouth,9,0.35);
    assert(fabs(number(s,"elapsed")-0.125) < 1e-6);
    assert(fabs(sprite_at(sprites,2)->opacity-0.5) < 1e-6);
    screenshot(sprites,"/tmp/02os-native-zero-portal-fade.png");
    script_lib_plymouth_on_boot_progress(s,plymouth,10.25,0.5);
    assert(number(s,"finished") == 1);
    assert(sprite_at(sprites,2)->opacity == 1);
    script_lib_plymouth_on_boot_progress(s,plymouth,30,0.6);
    assert(number(s,"finished") == 1);
    assert(sprite_at(sprites,2)->opacity == 1);
    assert(fabs(number(s,"elapsed")-0.25) < 1e-6);
    screenshot(sprites,"/tmp/02os-native-zero-portal-hold.png");
    script_lib_plymouth_on_display_message(s,plymouth,"Checking the filesystem");
    char *text=string(s,"message_text");
    assert(!strcmp(text,"Checking the filesystem")); free(text);
    script_lib_plymouth_on_hide_message(s,plymouth,"Checking the filesystem");
    text=string(s,"message_text"); assert(!strcmp(text,"")); free(text);
    script_lib_plymouth_on_display_password(s,plymouth,"Unlock encrypted disk",7);
    text=string(s,"entry_text"); assert(!strcmp(text,"•••••••")); free(text);
    assert(number(s,"prompt_active") == 1);
    assert(sprite_at(sprites,2)->opacity == 1);
    screenshot(sprites,"/tmp/02os-native-zero-portal-password.png");
    script_lib_plymouth_on_display_prompt(s,plymouth,"Secret", "password",true);
    text=string(s,"entry_text"); assert(!strcmp(text,"••••••••")); free(text);
    script_lib_plymouth_on_display_question(s,plymouth,"Continue?", "yes");
    text=string(s,"entry_text"); assert(!strcmp(text,"yes_")); free(text);
    script_lib_plymouth_on_display_normal(s,plymouth);
    assert(number(s,"prompt_active") == 0);
    for (ply_list_node_t *n=ply_list_get_first_node(sprites->sprite_list); n; n=ply_list_get_next_node(sprites->sprite_list,n)) {
        sprite_t *sprite=ply_list_node_get_data(n);
        if (sprite->z == 10) assert(sprite->opacity == 0);
    }
    sprites->max_width=800; sprites->max_height=600;
    script_lib_plymouth_on_display_hotplug(s,plymouth);
    script_lib_plymouth_on_refresh(s,plymouth);
    assert(number(s,"canvas_width") == 320);
    assert(sprite_at(sprites,2)->image);
    assert(sprite_at(sprites,2)->opacity == 1);
    screenshot(sprites,"/tmp/02os-native-zero-portal-800x600.png");
    script_lib_plymouth_on_display_question(s,plymouth,"Continue?", "yes");
    script_lib_plymouth_on_quit(s,plymouth);
    assert(sprite_at(sprites,2)->opacity == 1);
    assert(number(s,"prompt_active") == 0);
    for (ply_list_node_t *n=ply_list_get_first_node(sprites->sprite_list); n; n=ply_list_get_next_node(sprites->sprite_list,n)) {
        sprite_t *sprite=ply_list_node_get_data(n);
        if (sprite->z == 10) assert(sprite->opacity == 0);
    }
    puts("PASS native Zero Portal logo parse/assets, 0.25-second elapsed fade, repeated refreshes, nonmonotonic progress, static boot hold, password/question/message callbacks, resize, and quit");
    return 0;
}
