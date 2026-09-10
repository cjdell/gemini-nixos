/*
 * gemwl.c — a minimal wlroots 0.18 Wayland compositor for the Gemini PDA
 * whose single output IS the LK bootloader framebuffer, GPU-direct.
 *
 * DOUBLE-BUFFERED (since 2026-09-01): the Mali-T880 renders the composited
 * scene into a private SHADOW dma-buf (a panfrost BO), then a full-screen
 * GPU blit copies the finished frame into the LK framebuffer. The panel
 * only ever scans out COMPLETE frames — single-buffered rendering (scene
 * straight into the live fb) let the panel catch clear/draw partials
 * (cursor trails, black triangles while windows move/resize) and the next
 * frame's in-flight GPU work. The ~2-4 ms blit window is the only residual
 * artifact (a possible tear line). Set GEMWL_SINGLE_BUFFER=1 to force the
 * old direct-to-fb path for A/B.
 *
 * Chain:
 *   shadow: DRM_PANFROST_CREATE_BO -> DRM_PRIME_HANDLE_TO_FD
 *   -> custom wlr_buffer (get_dmabuf) -> custom allocator serves it to
 *   wlr_output's swapchain -> wlroots GLES2 renderer imports it as an
 *   EGL_LINUX_DMA_BUF_EXT image (patched Mesa panfrost) and renders the
 *   scene graph into it (the same import path the fb itself used).
 *   copy: a GLES3 COMPUTE SHADER texelFetches the shadow and imageStores
 *   the LK fb (glBindImageTexture on the gemfb dma-buf image). The
 *   compute path bypasses the tiler — fragment draws/blits into this
 *   1088x2160 LINEAR target silently clip to ~1024x1024 on this stack
 *   (tiler drops far bins, no faults; A/B'd 2026-09-01), while the
 *   compute copy covers the full frame at ~5.7 ms (1650 MB/s).
 *   /dev/gemfb ioctl(GEMFB_IOC_EXPORT) -> dma-buf fd of the LK fb region
 *
 * Zero CPU pixel movement — same chain as build/teapot/spin60.c but for
 * a whole compositor (scene graph, xdg-shell windows, input, cursor).
 *
 * Build on device (Debian trixie, after apt install libwlroots-0.18-dev
 * build-essential pkg-config):
 *   gcc -O2 -Wall -o gemwl gemwl.c -DWLR_USE_UNSTABLE \
 *       $(pkg-config --cflags --libs wlroots wayland-server xkbcommon) -lm
 *
 * Run (as root, on the device):
 *   PAN_MESA_DEBUG=noafbc ./gemwl [-t 90] [-s "cmd"] [-d]
 *     -t N   output transform (0/90/180/270; panel is physically
 *            landscape, fb is portrait -> 90 or 270 = landscape desktop)
 *     -s CMD run a Wayland client after starting
 *     -d     don't unbind fbcon at startup
 *
 * The fbcon console must be unbound while the compositor owns the fb
 * (it would redraw into the same memory): done by default at startup.
 */

#define _GNU_SOURCE
#include <assert.h>
#include <errno.h>
#include <fcntl.h>
#include <getopt.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <drm_fourcc.h>
#include <drm/drm.h>
#include <drm/panfrost_drm.h>

#include <wayland-server-core.h>
#include <wlr/backend.h>
#include <wlr/backend/interface.h>
#include <wlr/backend/libinput.h>
#include <wlr/backend/multi.h>
#include <wlr/backend/session.h>
#include <wlr/interfaces/wlr_buffer.h>
#include <wlr/interfaces/wlr_output.h>
#include <wlr/render/allocator.h>
#include <wlr/render/egl.h>
#include <wlr/render/gles2.h>
#include <wlr/render/wlr_renderer.h>

#include <EGL/egl.h>
#include <EGL/eglext.h>
#include <GLES2/gl2.h>
#include <GLES2/gl2ext.h>
#include <GLES3/gl31.h>   /* glDispatchCompute / glBindImageTexture / GL_COMPUTE_SHADER */
#include <wlr/types/wlr_compositor.h>
#include <wlr/types/wlr_cursor.h>
#include <wlr/types/wlr_touch.h>
#include <linux/input-event-codes.h>
#include <wlr/types/wlr_data_device.h>
#include <wlr/types/wlr_input_device.h>
#include <wlr/types/wlr_keyboard.h>
#include <wlr/types/wlr_output.h>
#include <wlr/types/wlr_output_layout.h>
#include <wlr/types/wlr_pointer.h>
#include <wlr/types/wlr_scene.h>
#include <wlr/types/wlr_seat.h>
#include <wlr/types/wlr_subcompositor.h>
#include <wlr/types/wlr_viewporter.h>
#include <wlr/types/wlr_xcursor_manager.h>
#include <wlr/types/wlr_xdg_shell.h>

/* Version identity (CORE RULE — docs/version-hygiene.md): header generated on
 * the HOST by build/gen-build-id.sh and pushed next to gemwl.c before the
 * on-device build.  Printed at startup so journal logs carry the build id. */
#if __has_include("gemini-build-id.h")
#include "gemini-build-id.h"
#else
#define GEMINI_BUILD_ID "unknown-nobuildid (run build/gen-build-id.sh before deploy)"
#endif
#include <wlr/util/log.h>
#include <xkbcommon/xkbcommon.h>

/* ------------------------------------------------------------------ */
/* LK framebuffer geometry (see docs/hardware.md): 1080x2160 at a      */
/* 1088-px row pitch (4352 B). The trailing 8 px/row are never shown.  */
/* ------------------------------------------------------------------ */
#define FB_W      1080
#define FB_H      2160
#define FB_PITCH  (1088 * 4)

#define GEMFB_IOC_MAGIC 'G'
#define GEMFB_IOC_EXPORT _IOW(GEMFB_IOC_MAGIC, 1, int)

/* ------------------------------------------------------------------ */
/* Double buffering: shadow buffer + GPU copy                          */
/*                                                                    */
/* The scene NEVER renders into the live LK framebuffer. It renders   */
/* into a private SHADOW dma-buf (a panfrost BO), then a full-frame   */
/* GLES3 COMPUTE SHADER copies it into the LK fb (the only memory the */
/* panel scans). All GPU — no CPU pixels, no cache coherency issues   */
/* (a CPU memcpy of a GPU-written BO would need dcache invalidation:  */
/* the Mali L2 flush alone is invisible to the CPU's cache, so reads  */
/* of a reused BO go stale; and it runs at ~102 MB/s here — dead).    */
/* A fragment blit was tried first but the midgard TILER silently     */
/* drops far bins for framebuffer-sized batches into this LINEAR      */
/* 1088x2160 target (no faults; coverage caps ~2.2 Mpx; A/B 2026-09-01). */
/* The compute path rasterizes without the tiler: full 1080x2160 at  */
/* ~5.7 ms (1650 MB/s), verified via t_blit in build/gltests.         */
/* ------------------------------------------------------------------ */
struct gemfb_blit {
	bool ready;
	int fb_fd;        /* LK fb dma-buf (copy target) */
	int shadow_fd;    /* shadow dma-buf (copy source) */
	EGLImageKHR fb_image;
	EGLImageKHR shadow_image;
	GLuint fb_tex;      /* texture over fb_image (imageStore target) */
	GLuint shadow_tex;  /* texture over shadow_image (texelFetch source) */
	GLuint cprog;       /* compute copy program */
	GLint c_src, c_size;
};

static PFNEGLCREATEIMAGEKHRPROC gemfb_eglCreateImageKHR;
static PFNGLEGLIMAGETARGETTEXTURE2DOESPROC gemfb_glEGLImageTargetTexture2DOES;

static const char *GEMFB_COPY_CS =
	"#version 310 es\n"
	"layout(local_size_x = 16, local_size_y = 8) in;\n"
	"layout(rgba8, binding = 0) uniform highp writeonly image2D dst;\n"
	"layout(binding = 1) uniform highp sampler2D src;\n"
	"uniform ivec2 fbSize;\n"
	"void main() {\n"
	"    ivec2 p = ivec2(gl_GlobalInvocationID.xy);\n"
	"    if (p.x >= fbSize.x || p.y >= fbSize.y) return;\n"
	"    /* The scene renders into the shadow as ABGR8888 (byte0=R) but the\n"
	"       LK OVL scans the fb as a8r8g8b8 (byte0=B).  texelFetch decodes\n"
	"       ABGR8888 to correct RGBA, so swizzle R/B on imageStore to land\n"
	"       B in byte0 — fixes the whole-desktop R/B colour swap.  (Colour\n"
	"       fix 2026-09-01; previously byte-faithful and R/B-swapped.)  */\n"
	"    imageStore(dst, p, texelFetch(src, p, 0).bgra);\n"
	"}\n";

/* Allocate the shadow buffer: a panfrost BO exported as a dma-buf.
 * The GPU renders the composited scene into it (wlroots' EGL import
 * path, exactly as it did into the fb), and the blit samples it.
 * Returns the dma-buf fd, or -1. */
static int gemfb_shadow_alloc(int drm_fd) {
	struct drm_panfrost_create_bo create = {
		.size = FB_PITCH * FB_H,
		.flags = 0,
	};
	if (ioctl(drm_fd, DRM_IOCTL_PANFROST_CREATE_BO, &create) != 0) {
		wlr_log(WLR_ERROR, "shadow: DRM_IOCTL_PANFROST_CREATE_BO failed: %s",
			strerror(errno));
		return -1;
	}
	struct drm_prime_handle prime = {
		.handle = create.handle,
		.flags = DRM_CLOEXEC,
	};
	if (ioctl(drm_fd, DRM_IOCTL_PRIME_HANDLE_TO_FD, &prime) != 0) {
		wlr_log(WLR_ERROR, "shadow: DRM_IOCTL_PRIME_HANDLE_TO_FD failed: %s",
			strerror(errno));
		return -1;
	}
	wlr_log(WLR_INFO, "shadow dma-buf: BO handle=%u size=%u -> fd=%d",
		create.handle, create.size, prime.fd);
	return prime.fd;
}

static EGLImageKHR gemfb_make_image(EGLDisplay display, int fd) {
	const EGLint attrs[] = {
		EGL_WIDTH, FB_W,
		EGL_HEIGHT, FB_H,
		EGL_LINUX_DRM_FOURCC_EXT, DRM_FORMAT_ABGR8888,
		EGL_DMA_BUF_PLANE0_FD_EXT, fd,
		EGL_DMA_BUF_PLANE0_OFFSET_EXT, 0,
		EGL_DMA_BUF_PLANE0_PITCH_EXT, FB_PITCH,
		EGL_NONE,
	};
	EGLImageKHR image = gemfb_eglCreateImageKHR(display, EGL_NO_CONTEXT,
		EGL_LINUX_DMA_BUF_EXT, NULL, attrs);
	if (image == EGL_NO_IMAGE_KHR) {
		wlr_log(WLR_ERROR, "blit: eglCreateImageKHR(fd=%d) failed: 0x%x",
			fd, eglGetError());
	}
	return image;
}

static void gemfb_tiler_warmup(GLuint shadow_tex);

/* Create the two EGL images, the shadow texture, the fb image texture
 * (the imageStore target) and the compute copy program. Must run with
 * the renderer's EGL context current. */
static bool gemfb_blit_init(struct gemfb_blit *blit, EGLDisplay display) {
	blit->fb_image = gemfb_make_image(display, blit->fb_fd);
	if (blit->fb_image == EGL_NO_IMAGE_KHR) {
		return false;
	}
	blit->shadow_image = gemfb_make_image(display, blit->shadow_fd);
	if (blit->shadow_image == EGL_NO_IMAGE_KHR) {
		return false;
	}

	/* target: texture over the LK fb (bound as a writeonly image2D) */
	glGenTextures(1, &blit->fb_tex);
	glBindTexture(GL_TEXTURE_2D, blit->fb_tex);
	gemfb_glEGLImageTargetTexture2DOES(GL_TEXTURE_2D, blit->fb_image);
	glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_NEAREST);
	glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_NEAREST);

	/* source: texture over the shadow buffer (texelFetch) */
	glGenTextures(1, &blit->shadow_tex);
	glBindTexture(GL_TEXTURE_2D, blit->shadow_tex);
	gemfb_glEGLImageTargetTexture2DOES(GL_TEXTURE_2D, blit->shadow_image);
	glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_NEAREST);
	glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_NEAREST);

	/* compute copy program (no tiler involved — see header comment) */
	GLuint cs = glCreateShader(GL_COMPUTE_SHADER);
	glShaderSource(cs, 1, &GEMFB_COPY_CS, NULL);
	glCompileShader(cs);
	GLint ok = GL_FALSE;
	glGetShaderiv(cs, GL_COMPILE_STATUS, &ok);
	if (!ok) {
		char log[512];
		glGetShaderInfoLog(cs, sizeof log, NULL, log);
		wlr_log(WLR_ERROR, "copy compute shader: %s", log);
	}
	blit->cprog = glCreateProgram();
	glAttachShader(blit->cprog, cs);
	glLinkProgram(blit->cprog);
	glGetProgramiv(blit->cprog, GL_LINK_STATUS, &ok);
	if (!ok) {
		char log[512];
		glGetProgramInfoLog(blit->cprog, sizeof log, NULL, log);
		wlr_log(WLR_ERROR, "copy compute program: %s", log);
	}
	glDeleteShader(cs);
	blit->c_src = glGetUniformLocation(blit->cprog, "src");
	blit->c_size = glGetUniformLocation(blit->cprog, "fbSize");

	/* The first real scene render into a fresh shadow target clips on the
	 * T880 (tiler first-batch bug) — warm it up so the desktop appears
	 * immediately instead of "revealing" as damage repaints. */
	gemfb_tiler_warmup(blit->shadow_tex);

	blit->ready = true;
	return true;
}

/* Warm up the T880 tiler: the FIRST tiler batch into a fresh
 * (framebuffer-size, hierarchy-mask) combination rasterizes only
 * ~1024x1024 (the desktop's "black until you move the mouse" symptom).
 * A throwaway full-frame draw into the shadow makes the first real
 * scene render clean. Verified with t_scene (build/gltests) 2026-09-02. */
static void gemfb_tiler_warmup(GLuint shadow_tex) {
	static const char *WVS =
		"attribute vec2 pos; void main(){ gl_Position=vec4(pos,0.0,1.0); }";
	static const char *WFS =
		"precision mediump float; void main(){ gl_FragColor=vec4(0,0,0,1); }";
	GLuint vs = glCreateShader(GL_VERTEX_SHADER);
	glShaderSource(vs, 1, &WVS, NULL);
	glCompileShader(vs);
	GLuint fs = glCreateShader(GL_FRAGMENT_SHADER);
	glShaderSource(fs, 1, &WFS, NULL);
	glCompileShader(fs);
	GLuint prog = glCreateProgram();
	glAttachShader(prog, vs);
	glAttachShader(prog, fs);
	glLinkProgram(prog);
	glDeleteShader(vs);
	glDeleteShader(fs);
	GLint a_pos = glGetAttribLocation(prog, "pos");

	GLuint fbo;
	glGenFramebuffers(1, &fbo);
	glBindFramebuffer(GL_FRAMEBUFFER, fbo);
	glFramebufferTexture2D(GL_FRAMEBUFFER, GL_COLOR_ATTACHMENT0, GL_TEXTURE_2D,
		shadow_tex, 0);
	glViewport(0, 0, FB_W, FB_H);
	glClearColor(0.0f, 0.0f, 0.0f, 1.0f);
	glClear(GL_COLOR_BUFFER_BIT);
	glUseProgram(prog);
	const float q[] = { -1, -1, 1, -1, -1, 1, 1, 1 };
	glVertexAttribPointer(a_pos, 2, GL_FLOAT, GL_FALSE, 0, q);
	glEnableVertexAttribArray(a_pos);
	glDrawArrays(GL_TRIANGLE_STRIP, 0, 4);
	glFinish();
	glDeleteFramebuffers(1, &fbo);
	glDeleteProgram(prog);
	wlr_log(WLR_INFO, "tiler warmup: full-frame draw into shadow done");
}

/* Copy the finished shadow frame into the LK fb (compute dispatch). Runs
 * with the renderer's EGL context current (gemfb_sync_gpu pattern) and
 * ends with glFinish so the fb write completes before the next frame
 * (and the copy finishes reading the shadow before the next scene render
 * overwrites it — same-context GL ordering already guarantees that). */
static void gemfb_blit_frame(struct wlr_renderer *renderer,
		struct gemfb_blit *blit) {
	if (!blit->ready) {
		return;
	}
	struct wlr_egl *egl = wlr_gles2_renderer_get_egl(renderer);
	EGLDisplay display = wlr_egl_get_display(egl);
	EGLContext context = wlr_egl_get_context(egl);
	EGLContext prev_context = eglGetCurrentContext();
	EGLSurface prev_draw = eglGetCurrentSurface(EGL_DRAW);
	EGLSurface prev_read = eglGetCurrentSurface(EGL_READ);
	if (eglMakeCurrent(display, EGL_NO_SURFACE, EGL_NO_SURFACE,
			context) != EGL_TRUE) {
		wlr_log(WLR_ERROR, "gemfb copy: eglMakeCurrent failed: 0x%x",
			eglGetError());
		return;
	}
	glUseProgram(blit->cprog);
	glUniform1i(blit->c_src, 1);
	glUniform2i(blit->c_size, FB_W, FB_H);
	glActiveTexture(GL_TEXTURE1);
	glBindTexture(GL_TEXTURE_2D, blit->shadow_tex);
	glBindImageTexture(0, blit->fb_tex, 0, GL_FALSE, 0, GL_WRITE_ONLY,
		GL_RGBA8);
	glDispatchCompute((FB_W + 15) / 16, (FB_H + 7) / 8, 1);
	glMemoryBarrier(GL_FRAMEBUFFER_BARRIER_BIT);
	glFinish();
	glBindImageTexture(0, 0, 0, GL_FALSE, 0, GL_WRITE_ONLY, GL_RGBA8);
	if (eglMakeCurrent(display, prev_draw, prev_read, prev_context) != EGL_TRUE) {
		wlr_log(WLR_ERROR, "gemfb copy: context restore failed: 0x%x",
			eglGetError());
	}
}

/* ------------------------------------------------------------------ */
/* gemfb wlr_buffer: wraps the LK-fb dma-buf (DMABUF caps only)        */
/* ------------------------------------------------------------------ */
struct gemfb_buffer {
	struct wlr_buffer base;
	int dma_fd; /* dup()'d per buffer, closed on destroy */
};

static bool gemfb_buffer_get_dmabuf(struct wlr_buffer *wlr_buffer,
		struct wlr_dmabuf_attributes *attribs) {
	struct gemfb_buffer *buf = wl_container_of(wlr_buffer, buf, base);
	*attribs = (struct wlr_dmabuf_attributes){
		.width = FB_W,
		.height = FB_H,
		.format = DRM_FORMAT_ABGR8888, /* renders; NOTE: ARGB8888 renders BLACK
					  via wlroots gles2 on this stack (A/B 2026-09-01: wlroots maps
					  AR24 -> GL_BGRA_EXT, panfrost silently fails; direct
					  eglCreateImageKHR ARGB8888 works — see build/gltests).
					  Consequence: the OVL reads byte0=B so the composited frame
					  is R/B-swapped on glass; color fix = make wlroots render
					  AR24 or swizzle, NOT yet solved. */
		.modifier = DRM_FORMAT_MOD_LINEAR,
		.n_planes = 1,
		.fd = { buf->dma_fd, -1, -1, -1 },
		.offset = { 0, 0, 0, 0 },
		.stride = { FB_PITCH, 0, 0, 0 },
	};
	return true;
}

static void gemfb_buffer_destroy(struct wlr_buffer *wlr_buffer) {
	struct gemfb_buffer *buf = wl_container_of(wlr_buffer, buf, base);
	close(buf->dma_fd);
	free(buf);
}

static const struct wlr_buffer_impl gemfb_buffer_impl = {
	.destroy = gemfb_buffer_destroy,
	.get_dmabuf = gemfb_buffer_get_dmabuf,
};

static struct wlr_buffer *gemfb_buffer_create(int dma_fd) {
	struct gemfb_buffer *buf = calloc(1, sizeof(*buf));
	if (buf == NULL) {
		return NULL;
	}
	buf->dma_fd = dup(dma_fd);
	if (buf->dma_fd < 0) {
		free(buf);
		return NULL;
	}
	wlr_buffer_init(&buf->base, &gemfb_buffer_impl, FB_W, FB_H);
	return &buf->base;
}

/* ------------------------------------------------------------------ */
/* gemfb allocator: always serves the LK-fb buffer (the output has     */
/* exactly one "scanout" buffer — the framebuffer itself).             */
/* ------------------------------------------------------------------ */
struct gemfb_allocator {
	struct wlr_allocator base;
	int dma_fd;
};

static struct wlr_buffer *gemfb_allocator_create_buffer(
		struct wlr_allocator *alloc, int width, int height,
		const struct wlr_drm_format *format) {
	struct gemfb_allocator *allocator = wl_container_of(alloc, allocator, base);
	return gemfb_buffer_create(allocator->dma_fd);
}

static void gemfb_allocator_destroy(struct wlr_allocator *alloc) {
	struct gemfb_allocator *allocator = wl_container_of(alloc, allocator, base);
	close(allocator->dma_fd);
	free(allocator);
}

static const struct wlr_allocator_interface gemfb_allocator_impl = {
	.create_buffer = gemfb_allocator_create_buffer,
	.destroy = gemfb_allocator_destroy,
};

static struct wlr_allocator *gemfb_allocator_create(int dma_fd) {
	struct gemfb_allocator *allocator = calloc(1, sizeof(*allocator));
	if (allocator == NULL) {
		return NULL;
	}
	allocator->dma_fd = dup(dma_fd);
	if (allocator->dma_fd < 0) {
		free(allocator);
		return NULL;
	}
	wlr_allocator_init(&allocator->base, &gemfb_allocator_impl,
		WLR_BUFFER_CAP_DMABUF);
	return &allocator->base;
}

static void clear_framebuffer(void) {
	/* The wlroots scene background is transparent; regions the scene
	 * never renders would show stale bootloader content. Zero the fb
	 * once at startup for a clean black background. */
	int fd = open("/dev/fb0", O_RDWR);
	if (fd < 0) {
		wlr_log(WLR_ERROR, "open /dev/fb0 failed: %s", strerror(errno));
		return;
	}
	void *map = mmap(NULL, FB_PITCH * FB_H, PROT_READ | PROT_WRITE,
		MAP_SHARED, fd, 0);
	if (map == MAP_FAILED) {
		wlr_log(WLR_ERROR, "mmap /dev/fb0 failed: %s", strerror(errno));
		close(fd);
		return;
	}
	memset(map, 0, FB_PITCH * FB_H);
	munmap(map, FB_PITCH * FB_H);
	close(fd);
}

/* ------------------------------------------------------------------ */
/* gemfb output: no presentation work — rendering already happened     */
/* straight into the framebuffer via the swapchain buffer. A 60 Hz     */
/* timer drives the frame event (there is no vblank on the LK fb) —    */
/* the headless-backend pattern.                                       */
/* ------------------------------------------------------------------ */
struct gemfb_output {
	struct wlr_output wlr_output;
	struct wl_event_source *frame_timer;
};

/* wlr_output_send_frame is internal to wlroots but exported; the
 * headless backend uses it to drive its frame events. */
extern void wlr_output_send_frame(struct wlr_output *output);

#define GEMFB_FRAME_DELAY_MS 16 /* 60 Hz (wl_event_source_timer_update takes MILLISECONDS!) */

static int gemfb_signal_frame(void *data) {
	struct gemfb_output *output = data;
	wlr_output_send_frame(&output->wlr_output);
	return 0;
}

static bool gemfb_output_test(struct wlr_output *wlr_output,
		const struct wlr_output_state *state) {
	return true;
}

static bool gemfb_output_commit(struct wlr_output *wlr_output,
		const struct wlr_output_state *state) {
	struct gemfb_output *output = wl_container_of(wlr_output, output, wlr_output);
	/* Re-arm the frame timer for the next frame. When the scene has no
	 * damage, wlr_scene_output_commit does not commit and the timer is
	 * not re-armed -> the compositor idles. */
	wl_event_source_timer_update(output->frame_timer, GEMFB_FRAME_DELAY_MS);
	return true;
}

static void gemfb_output_destroy(struct wlr_output *wlr_output) {
	struct gemfb_output *output = wl_container_of(wlr_output, output, wlr_output);
	wl_event_source_remove(output->frame_timer);
	free(output);
}

static const struct wlr_output_impl gemfb_output_impl = {
	.test = gemfb_output_test,
	.commit = gemfb_output_commit,
	.destroy = gemfb_output_destroy,
};

/* ------------------------------------------------------------------ */
/* gemfb backend                                                       */
/* ------------------------------------------------------------------ */
struct gemfb_backend {
	struct wlr_backend base;
	struct wl_event_loop *event_loop;
	int drm_fd;   /* panfrost render node (renderer/EGL) */
	int dma_fd;   /* LK-fb dma-buf (master fd) */
	uint32_t transform;
	bool unbind_fbcon;
};

static int gemfb_backend_get_drm_fd(struct wlr_backend *backend) {
	struct gemfb_backend *b = wl_container_of(backend, b, base);
	return b->drm_fd;
}

static uint32_t gemfb_backend_get_buffer_caps(struct wlr_backend *backend) {
	return WLR_BUFFER_CAP_DMABUF;
}

static void unbind_fbcons(void) {
	for (int i = 0; i < 4; i++) {
		char path[64];
		snprintf(path, sizeof(path),
			"/sys/class/vtconsole/vtcon%d/bind", i);
		FILE *f = fopen(path, "w");
		if (f) {
			fprintf(f, "0");
			fclose(f);
		}
	}
}

static bool gemfb_backend_start(struct wlr_backend *backend) {
	struct gemfb_backend *b = wl_container_of(backend, b, base);

	if (b->unbind_fbcon) {
		unbind_fbcons();
	}
	clear_framebuffer();

	struct gemfb_output *output = calloc(1, sizeof(*output));
	if (output == NULL) {
		return false;
	}

	struct wlr_output_state state;
	wlr_output_state_init(&state);
	wlr_output_state_set_enabled(&state, true);
	wlr_output_state_set_custom_mode(&state, FB_W, FB_H, 60000);
	wlr_output_state_set_transform(&state, (enum wl_output_transform)b->transform);
	wlr_output_state_set_render_format(&state, DRM_FORMAT_ABGR8888);
	/* ABGR8888 (renders; R/B-swapped on the OVL).  ARGB8888 gives a
	 * black output through wlroots gles2 (see gemfb_buffer_get_dmabuf). */
	/* ARGB8888 = a8r8g8b8 memory [B,G,R,A] — matches the LK OVL byte
	 * order; ABGR8888 would render the whole desktop R/B-swapped. */
	wlr_output_init(&output->wlr_output, backend, &gemfb_output_impl,
		b->event_loop, &state);
	wlr_output_state_finish(&state);

	output->frame_timer = wl_event_loop_add_timer(b->event_loop,
		gemfb_signal_frame, output);
	if (output->frame_timer == NULL) {
		free(output);
		return false;
	}

	struct wlr_output *wlr_output = &output->wlr_output;
	wlr_output->phys_width = 70;  /* ~5.99" FHD panel, 1080x2160 */
	wlr_output->phys_height = 140;
	wlr_output_set_name(wlr_output, "gemfb");
	wlr_output_set_description(wlr_output, "LK framebuffer (GPU-direct)");

	wl_signal_emit_mutable(&backend->events.new_output, wlr_output);
	return true;
}

static void gemfb_backend_destroy(struct wlr_backend *backend) {
	struct gemfb_backend *b = wl_container_of(backend, b, base);
	wlr_backend_finish(backend);
	if (b->dma_fd >= 0) {
		close(b->dma_fd);
	}
	if (b->drm_fd >= 0) {
		close(b->drm_fd);
	}
	free(b);
}

static const struct wlr_backend_impl gemfb_backend_impl = {
	.start = gemfb_backend_start,
	.destroy = gemfb_backend_destroy,
	.get_drm_fd = gemfb_backend_get_drm_fd,
	.get_buffer_caps = gemfb_backend_get_buffer_caps,
	.test = NULL,
	.commit = NULL,
};

static struct wlr_backend *gemfb_backend_create(struct wl_event_loop *loop,
		int drm_fd, int dma_fd, uint32_t transform, bool unbind_fbcon) {
	struct gemfb_backend *b = calloc(1, sizeof(*b));
	if (b == NULL) {
		return NULL;
	}
	wlr_backend_init(&b->base, &gemfb_backend_impl);
	b->event_loop = loop;
	b->drm_fd = dup(drm_fd);
	b->dma_fd = dup(dma_fd);
	b->transform = transform;
	b->unbind_fbcon = unbind_fbcon;
	return &b->base;
}

/* ------------------------------------------------------------------ */
/* Server (tinywl-derived: xdg-shell, scene, seat, cursor, keyboard)   */
/* ------------------------------------------------------------------ */
struct gemwl_server {
	struct wl_display *wl_display;
	struct wlr_backend *backend;
	struct wlr_renderer *renderer;
	struct wlr_allocator *allocator;
	struct wlr_scene *scene;
	struct wlr_scene_output_layout *scene_layout;

	struct wlr_xdg_shell *xdg_shell;
	struct wl_listener new_xdg_toplevel;
	struct wl_listener new_xdg_popup;
	struct wl_list toplevels;

	struct wlr_cursor *cursor;
	struct wlr_xcursor_manager *cursor_mgr;
	struct wl_listener cursor_motion;
	struct wl_listener cursor_motion_absolute;
	struct wl_listener cursor_button;
	struct wl_listener cursor_axis;
	struct wl_listener cursor_frame;

	/* touchscreen handling (see the touch section below): by default
	 * every finger is forwarded to the seat as a real wl_touch device
	 * (no cursor); GEMWL_TOUCH_POINTER_EMU=1 restores the legacy
	 * first-finger-as-pointer emulation. */
	bool touch_pointer_emu;  /* legacy pointer emulation (env opt-in) */
	bool touch_emulating;    /* emu mode: a finger is driving the pointer */
	int32_t touch_emulated_id; /* touch_id of the finger being emulated */
	bool has_touch;          /* a touch device is attached (seat caps) */
	struct wl_listener touch_down;
	struct wl_listener touch_motion;
	struct wl_listener touch_up;

	struct wlr_seat *seat;
	struct wl_listener new_input;
	struct wl_listener request_cursor;
	struct wl_listener request_set_selection;
	struct wl_list keyboards;

	struct wlr_output_layout *output_layout;
	struct wl_list outputs;
	struct wl_listener new_output;

	/* double-buffered output state (see gemfb_blit above) */
	struct gemfb_blit blit;
	bool double_buffer;
	int fb_dma_fd;     /* LK fb dma-buf (blit target) */
	int shadow_dma_fd; /* shadow dma-buf (blit source) */
};

struct gemwl_output {
	struct wl_list link;
	struct gemwl_server *server;
	struct wlr_output *wlr_output;
	struct wl_listener frame;
	struct wl_listener request_state;
	struct wl_listener destroy;
};

struct gemwl_toplevel {
	struct wl_list link;
	struct gemwl_server *server;
	struct wlr_xdg_toplevel *xdg_toplevel;
	struct wlr_scene_tree *scene_tree;
	struct wl_listener map;
	struct wl_listener unmap;
	struct wl_listener commit;
	struct wl_listener destroy;
	struct wl_listener request_fullscreen;
};

struct gemwl_popup {
	struct wlr_xdg_popup *xdg_popup;
	struct wl_listener commit;
	struct wl_listener destroy;
};

struct gemwl_keyboard {
	struct wl_list link;
	struct gemwl_server *server;
	struct wlr_keyboard *wlr_keyboard;
	struct wl_listener modifiers;
	struct wl_listener key;
	struct wl_listener destroy;
};

static void focus_toplevel(struct gemwl_toplevel *toplevel,
		struct wlr_surface *surface) {
	if (toplevel == NULL) {
		return;
	}
	struct gemwl_server *server = toplevel->server;
	struct wlr_seat *seat = server->seat;
	struct wlr_surface *prev_surface = seat->keyboard_state.focused_surface;
	if (prev_surface == surface) {
		return;
	}
	if (prev_surface) {
		struct wlr_xdg_toplevel *prev_toplevel =
			wlr_xdg_toplevel_try_from_wlr_surface(prev_surface);
		if (prev_toplevel != NULL) {
			wlr_xdg_toplevel_set_activated(prev_toplevel, false);
		}
	}
	struct wlr_keyboard *keyboard = wlr_seat_get_keyboard(seat);
	wlr_scene_node_raise_to_top(&toplevel->scene_tree->node);
	wl_list_remove(&toplevel->link);
	wl_list_insert(&server->toplevels, &toplevel->link);
	wlr_xdg_toplevel_set_activated(toplevel->xdg_toplevel, true);
	if (keyboard != NULL) {
		wlr_seat_keyboard_notify_enter(seat,
			toplevel->xdg_toplevel->base->surface,
			keyboard->keycodes, keyboard->num_keycodes,
			&keyboard->modifiers);
	}
}

static void keyboard_handle_modifiers(struct wl_listener *listener, void *data) {
	struct gemwl_keyboard *keyboard =
		wl_container_of(listener, keyboard, modifiers);
	wlr_seat_set_keyboard(keyboard->server->seat, keyboard->wlr_keyboard);
	wlr_seat_keyboard_notify_modifiers(keyboard->server->seat,
		&keyboard->wlr_keyboard->modifiers);
}

/* No compositor keybindings: this is a single-client desktop (KWin
 * handles its own shortcuts). Forward EVERY key to the client — the
 * old tinywl Alt+Esc binding terminated the compositor during input
 * testing. */
static void keyboard_handle_key(struct wl_listener *listener, void *data) {
	struct gemwl_keyboard *keyboard =
		wl_container_of(listener, keyboard, key);
	struct gemwl_server *server = keyboard->server;
	struct wlr_seat *seat = server->seat;
	struct wlr_keyboard_key_event *event = data;

	uint32_t keycode = event->keycode + 8;
	const xkb_keysym_t *syms;
	int nsyms = xkb_state_key_get_syms(
		keyboard->wlr_keyboard->xkb_state, keycode, &syms);
	if (event->state == WL_KEYBOARD_KEY_STATE_PRESSED) {
		wlr_log(WLR_INFO, "gemwl input: key PRESS keycode=%u keysym=0x%x",
			event->keycode, nsyms > 0 ? syms[0] : 0);
	} else {
		wlr_log(WLR_INFO, "gemwl input: key release keycode=%u", event->keycode);
	}

	wlr_seat_set_keyboard(seat, keyboard->wlr_keyboard);
	wlr_seat_keyboard_notify_key(seat, event->time_msec,
		event->keycode, event->state);
}

static void keyboard_handle_destroy(struct wl_listener *listener, void *data) {
	struct gemwl_keyboard *keyboard =
		wl_container_of(listener, keyboard, destroy);
	wl_list_remove(&keyboard->modifiers.link);
	wl_list_remove(&keyboard->key.link);
	wl_list_remove(&keyboard->destroy.link);
	wl_list_remove(&keyboard->link);
	free(keyboard);
}

static void server_new_keyboard(struct gemwl_server *server,
		struct wlr_input_device *device) {
	struct wlr_keyboard *wlr_keyboard = wlr_keyboard_from_input_device(device);

	struct gemwl_keyboard *keyboard = calloc(1, sizeof(*keyboard));
	keyboard->server = server;
	keyboard->wlr_keyboard = wlr_keyboard;

	struct xkb_context *context = xkb_context_new(XKB_CONTEXT_NO_FLAGS);
	/* Gemini PDA keyboard: layout "gemini" (symbols/gemini, UK = default
	 * "basic" block; US = variant "us"). Model pc105 keeps the
	 * KEY_*->keycode mapping standard. Override with GEMWL_XKB_LAYOUT /
	 * GEMWL_XKB_VARIANT env (X11 layout now also defaults to gemini,
	 * see /etc/default/keyboard). */
	struct xkb_rule_names rules = {
		.model = "pc105",
		.layout = getenv("GEMWL_XKB_LAYOUT") ? : "gemini",
		.variant = getenv("GEMWL_XKB_VARIANT") ? : NULL,
	};
	struct xkb_keymap *keymap = xkb_keymap_new_from_names(context, &rules,
		XKB_KEYMAP_COMPILE_NO_FLAGS);
	if (keymap == NULL) {
		wlr_log(WLR_ERROR, "failed to compile keymap pc105/%s; using default",
			rules.layout);
		keymap = xkb_keymap_new_from_names(context, NULL,
			XKB_KEYMAP_COMPILE_NO_FLAGS);
	}
	wlr_keyboard_set_keymap(wlr_keyboard, keymap);
	wlr_log(WLR_INFO, "keyboard keymap: model=pc105 layout=%s variant=%s",
		rules.layout, rules.variant ? rules.variant : "(default)");
	xkb_keymap_unref(keymap);
	xkb_context_unref(context);
	wlr_keyboard_set_repeat_info(wlr_keyboard, 25, 600);

	keyboard->modifiers.notify = keyboard_handle_modifiers;
	wl_signal_add(&wlr_keyboard->events.modifiers, &keyboard->modifiers);
	keyboard->key.notify = keyboard_handle_key;
	wl_signal_add(&wlr_keyboard->events.key, &keyboard->key);
	keyboard->destroy.notify = keyboard_handle_destroy;
	wl_signal_add(&device->events.destroy, &keyboard->destroy);

	wlr_seat_set_keyboard(server->seat, wlr_keyboard);
	wl_list_insert(&server->keyboards, &keyboard->link);
}

static void server_new_pointer(struct gemwl_server *server,
		struct wlr_input_device *device) {
	wlr_cursor_attach_input_device(server->cursor, device);
}

/* ------------------------------------------------------------------
 * Touchscreen -> real multitouch (wl_touch protocol, NO cursor).
 *
 * The Novatek NT36772 kernel driver (delta
 * drivers/input/touchscreen/novatek-nt36xxx.c) exposes a 10-point
 * Protocol-B device and already maps the sensor into the landscape
 * output space; wlroots normalizes each finger to 0..1. gemwl
 * forwards EVERY finger to the seat via wlr_seat_touch_notify_*, so
 * the nested session's wlroots wayland backend (phoc 0.54 /
 * wlroots 0.19.3 and labwc 0.8.3 / wlroots 0.18.2 both carry the
 * touch path) synthesizes its own wlr_touch device and the phosh
 * apps get a REAL wl_touch: taps, one-finger drags and multi-finger
 * gestures (pinch/zoom in GTK4/WebKit apps) — with no on-screen
 * cursor, because touch never touches the pointer.
 *
 * Coordinates: normalized (0..1) output space -> output layout box ->
 * scene hit-test -> surface-local, the same transform the pointer
 * path uses. The nested toplevel is full-screen at scale 1, so the
 * wl_touch protocol's "coordinates relative to the surface from the
 * down event" contract holds for the whole gesture.
 *
 * [changed 2026-09-10: previously the first finger was emulated as
 * an absolute pointer (warp + synthetic BTN_LEFT) because "nothing
 * in the nested LXQt stack consumes wl_touch". That was true for the
 * LXQt-only era; the phosh stack's phoc consumes wl_touch natively
 * (seat_add_touch -> phoc cursor -> wlr_seat_touch_notify_* + its own
 * zoom/swipe gesture recognizers), and userspace wanted the real
 * device for GNOME app gestures. The old behaviour is kept as the
 * GEMWL_TOUCH_POINTER_EMU=1 fallback for A/B on glass.]
 * ------------------------------------------------------------------ */
static void pointer_focus_and_send(struct gemwl_server *server, uint32_t time);

static struct wlr_output *gemwl_first_output(struct gemwl_server *server) {
	struct gemwl_output *o;
	wl_list_for_each(o, &server->outputs, link) {
		if (o->wlr_output->enabled)
			return o->wlr_output;
	}
	return NULL;
}

/* Map a normalized (0..1) output-space point to the surface under it;
 * *sx/*sy receive the surface-local coordinates. NULL if there is
 * nothing there to touch. */
static struct wlr_surface *touch_surface_at(struct gemwl_server *server,
		double nx, double ny, double *sx, double *sy) {
	struct wlr_output *output = gemwl_first_output(server);
	if (output == NULL)
		return NULL;
	struct wlr_box box;
	wlr_output_layout_get_box(server->output_layout, output, &box);
	if (box.width <= 0 || box.height <= 0)
		return NULL;
	double lx = box.x + nx * box.width;
	double ly = box.y + ny * box.height;
	struct wlr_scene_node *node =
		wlr_scene_node_at(&server->scene->tree.node, lx, ly, &lx, &ly);
	if (node == NULL || node->type != WLR_SCENE_NODE_BUFFER)
		return NULL;
	struct wlr_scene_surface *scene_surface =
		wlr_scene_surface_try_from_buffer(wlr_scene_buffer_from_node(node));
	if (scene_surface == NULL)
		return NULL;
	*sx = lx;
	*sy = ly;
	return scene_surface->surface;
}

/* A touch point is only valid if the nested client has requested
 * wl_seat.get_touch() (wlroots logs an error on every down otherwise).
 * phoc/labwc request it as soon as they see the TOUCH seat capability,
 * so this only matters before the session's toplevel binds the seat. */
static bool client_has_touch(struct gemwl_server *server,
		struct wlr_surface *surface) {
	struct wl_client *client = wl_resource_get_client(surface->resource);
	struct wlr_seat_client *sc =
		wlr_seat_client_for_wl_client(server->seat, client);
	return sc != NULL && !wl_list_empty(&sc->touches);
}

/* --- legacy pointer emulation (GEMWL_TOUCH_POINTER_EMU=1 only) --- */

static void touch_emulate_motion(struct gemwl_server *server, uint32_t time,
		double nx, double ny) {
	struct wlr_output *output = gemwl_first_output(server);
	struct wlr_box box;
	if (output == NULL)
		return;
	wlr_output_layout_get_box(server->output_layout, output, &box);
	if (box.width <= 0 || box.height <= 0)
		return;
	wlr_cursor_warp(server->cursor, NULL,
			box.x + nx * box.width, box.y + ny * box.height);
	pointer_focus_and_send(server, time);
}

static void touch_emu_down(struct gemwl_server *server,
		struct wlr_touch_down_event *event) {
	if (server->touch_emulating)
		return; /* only the first finger is emulated (no gestures yet) */
	server->touch_emulating = true;
	server->touch_emulated_id = event->touch_id;
	touch_emulate_motion(server, event->time_msec, event->x, event->y);
	wlr_log(WLR_INFO, "gemwl input: touch DOWN (%.2f,%.2f) -> click at (%.0f,%.0f)",
		event->x, event->y, server->cursor->x, server->cursor->y);
	wlr_seat_pointer_notify_button(server->seat, event->time_msec,
		BTN_LEFT, WL_POINTER_BUTTON_STATE_PRESSED);
	/* Commit the synthesized click: real pointer devices batch their
	 * events and send wl_pointer.frame (server_cursor_frame); the touch
	 * path called notify_button directly with no frame, so clients
	 * (labwc->Qt) never committed the click — only enter/motion did.
	 * A frame after the press (and after motion/up) makes it register.
	 * Added 2026-09-04 (handover #288 next-session fix #1). */
	wlr_seat_pointer_notify_frame(server->seat);
}

static void touch_emu_motion(struct gemwl_server *server,
		struct wlr_touch_motion_event *event) {
	if (!server->touch_emulating ||
	    event->touch_id != server->touch_emulated_id)
		return; /* second+ fingers are ignored */
	touch_emulate_motion(server, event->time_msec, event->x, event->y);
	/* Frame after touch motion so the client commits the pointer move
	 * (same reason as touch_handle_down). */
	wlr_seat_pointer_notify_frame(server->seat);
}

static void touch_emu_up(struct gemwl_server *server,
		struct wlr_touch_up_event *event) {
	if (!server->touch_emulating ||
	    event->touch_id != server->touch_emulated_id)
		return; /* a non-emulated finger lifted: nothing to release */
	server->touch_emulating = false;
	server->touch_emulated_id = -1;
	wlr_log(WLR_INFO, "gemwl input: touch UP (release)");
	wlr_seat_pointer_notify_button(server->seat, event->time_msec,
		BTN_LEFT, WL_POINTER_BUTTON_STATE_RELEASED);
	/* Frame after the release, to commit the button transition. */
	wlr_seat_pointer_notify_frame(server->seat);
}

/* --- real wl_touch forwarding (default) --- */

static void touch_handle_down(struct wl_listener *listener, void *data) {
	struct gemwl_server *server =
		wl_container_of(listener, server, touch_down);
	struct wlr_touch_down_event *event = data;
	if (server->touch_pointer_emu) {
		touch_emu_down(server, event);
		return;
	}
	double sx, sy;
	struct wlr_surface *surface =
		touch_surface_at(server, event->x, event->y, &sx, &sy);
	if (surface == NULL) {
		wlr_log(WLR_INFO, "gemwl input: touch DOWN id=%d (%.2f,%.2f) - no surface",
			event->touch_id, event->x, event->y);
		return;
	}
	if (!client_has_touch(server, surface)) {
		wlr_log(WLR_DEBUG, "gemwl input: touch DOWN id=%d - client has no wl_touch yet, dropped",
			event->touch_id);
		return;
	}
	wlr_log(WLR_INFO, "gemwl input: touch DOWN id=%d (%.2f,%.2f) -> surface-local (%.0f,%.0f)",
		event->touch_id, event->x, event->y, sx, sy);
	wlr_seat_touch_notify_down(server->seat, surface, event->time_msec,
		event->touch_id, sx, sy);
	wlr_seat_touch_notify_frame(server->seat);
}

static void touch_handle_motion(struct wl_listener *listener, void *data) {
	struct gemwl_server *server =
		wl_container_of(listener, server, touch_motion);
	struct wlr_touch_motion_event *event = data;
	if (server->touch_pointer_emu) {
		touch_emu_motion(server, event);
		return;
	}
	if (!wlr_seat_touch_get_point(server->seat, event->touch_id))
		return; /* no point for this finger (its down was dropped) */
	double sx, sy;
	struct wlr_surface *surface =
		touch_surface_at(server, event->x, event->y, &sx, &sy);
	if (surface != NULL) {
		wlr_seat_touch_notify_motion(server->seat, event->time_msec,
			event->touch_id, sx, sy);
		wlr_seat_touch_notify_frame(server->seat);
	}
}

static void touch_handle_up(struct wl_listener *listener, void *data) {
	struct gemwl_server *server =
		wl_container_of(listener, server, touch_up);
	struct wlr_touch_up_event *event = data;
	if (server->touch_pointer_emu) {
		touch_emu_up(server, event);
		return;
	}
	wlr_log(WLR_INFO, "gemwl input: touch UP id=%d", event->touch_id);
	wlr_seat_touch_notify_up(server->seat, event->time_msec, event->touch_id);
	wlr_seat_touch_notify_frame(server->seat);
}

static void server_new_touch(struct gemwl_server *server,
		struct wlr_input_device *device) {
	struct wlr_touch *touch = wlr_touch_from_input_device(device);
	server->touch_down.notify = touch_handle_down;
	wl_signal_add(&touch->events.down, &server->touch_down);
	server->touch_motion.notify = touch_handle_motion;
	wl_signal_add(&touch->events.motion, &server->touch_motion);
	server->touch_up.notify = touch_handle_up;
	wl_signal_add(&touch->events.up, &server->touch_up);
	server->has_touch = true;
	wlr_log(WLR_INFO, "gemwl input: touchscreen attached (%s): %s",
		server->touch_pointer_emu ? "pointer emulation" : "wl_touch forwarding",
		device->name);
}

static void server_new_input(struct wl_listener *listener, void *data) {
	struct gemwl_server *server =
		wl_container_of(listener, server, new_input);
	struct wlr_input_device *device = data;
	switch (device->type) {
	case WLR_INPUT_DEVICE_KEYBOARD:
		server_new_keyboard(server, device);
		break;
	case WLR_INPUT_DEVICE_POINTER:
		server_new_pointer(server, device);
		break;
	case WLR_INPUT_DEVICE_TOUCH:
		server_new_touch(server, device);
		break;
	default:
		break;
	}
	uint32_t caps = WL_SEAT_CAPABILITY_POINTER;
	if (!wl_list_empty(&server->keyboards)) {
		caps |= WL_SEAT_CAPABILITY_KEYBOARD;
	}
	/* Advertise touch only when we forward it: in emu mode the fingers
	 * drive the pointer, and a dead touch device would confuse clients. */
	if (server->has_touch && !server->touch_pointer_emu) {
		caps |= WL_SEAT_CAPABILITY_TOUCH;
	}
	wlr_seat_set_capabilities(server->seat, caps);
}

static void seat_request_cursor(struct wl_listener *listener, void *data) {
	struct gemwl_server *server =
		wl_container_of(listener, server, request_cursor);
	struct wlr_seat_pointer_request_set_cursor_event *event = data;
	struct wlr_seat_client *focused_client =
		server->seat->pointer_state.focused_client;
	if (focused_client == event->seat_client) {
		wlr_cursor_set_surface(server->cursor, event->surface,
			event->hotspot_x, event->hotspot_y);
	}
}

static void seat_request_set_selection(struct wl_listener *listener, void *data) {
	struct gemwl_server *server =
		wl_container_of(listener, server, request_set_selection);
	struct wlr_seat_request_set_selection_event *event = data;
	wlr_seat_set_selection(server->seat, event->source,
		event->serial);
}

/* wlroots 0.18: wlr_seat_pointer_notify_{enter,motion}() take
 * SURFACE-LOCAL coordinates (hit-test result), not layout coords and
 * not deltas. gemwl (old tinywl API) passed raw libinput deltas, which
 * made the client pointer jump to (delta_x, delta_y) near the origin
 * on every event. Hit-test the scene like tinywl 0.18.2 does. */
static void pointer_focus_and_send(struct gemwl_server *server, uint32_t time) {
	double lx = server->cursor->x, ly = server->cursor->y;
	/* hit-test the scene (tinywl 0.18.2 pattern) */
	struct wlr_scene_node *node =
		wlr_scene_node_at(&server->scene->tree.node, lx, ly, &lx, &ly);
	struct wlr_surface *surface = NULL;
	if (node != NULL && node->type == WLR_SCENE_NODE_BUFFER) {
		struct wlr_scene_surface *scene_surface = wlr_scene_surface_try_from_buffer(
			wlr_scene_buffer_from_node(node));
		surface = scene_surface ? scene_surface->surface : NULL;
	}
	if (surface != NULL) {
		wlr_seat_pointer_notify_enter(server->seat, surface, lx, ly);
		wlr_seat_pointer_notify_motion(server->seat, time, lx, ly);
	} else {
		wlr_seat_pointer_clear_focus(server->seat);
	}
}

static void log_cursor_pos(struct gemwl_server *server, const char *what) {
	static struct timespec last = {0, 0};
	struct timespec now;
	clock_gettime(CLOCK_MONOTONIC, &now);
	/* throttle: ~2 logs/sec so the journal stays usable */
	long long diff_ns = (long long)(now.tv_sec - last.tv_sec) * 1000000000LL
		+ (now.tv_nsec - last.tv_nsec);
	if (diff_ns < 500000000LL) {
		return;
	}
	last = now;
	wlr_log(WLR_INFO, "gemwl input: %s cursor at (%.0f, %.0f)", what,
		server->cursor->x, server->cursor->y);
}

static void server_cursor_motion(struct wl_listener *listener, void *data) {
	struct gemwl_server *server =
		wl_container_of(listener, server, cursor_motion);
	struct wlr_pointer_motion_event *event = data;
	wlr_cursor_move(server->cursor, &event->pointer->base,
		event->delta_x, event->delta_y);
	pointer_focus_and_send(server, event->time_msec);
	log_cursor_pos(server, "motion");
}

static void server_cursor_motion_absolute(struct wl_listener *listener, void *data) {
	struct gemwl_server *server =
		wl_container_of(listener, server, cursor_motion_absolute);
	struct wlr_pointer_motion_absolute_event *event = data;
	wlr_cursor_warp_absolute(server->cursor, &event->pointer->base,
		event->x, event->y);
	pointer_focus_and_send(server, event->time_msec);
	log_cursor_pos(server, "motion-abs");
}

static void server_cursor_button(struct wl_listener *listener, void *data) {
	struct gemwl_server *server =
		wl_container_of(listener, server, cursor_button);
	struct wlr_pointer_button_event *event = data;
	wlr_log(WLR_INFO, "gemwl input: button %u %s at (%.0f, %.0f)",
		event->button,
		event->state == WL_POINTER_BUTTON_STATE_PRESSED ? "PRESS" : "release",
		server->cursor->x, server->cursor->y);
	wlr_seat_pointer_notify_button(server->seat, event->time_msec,
		event->button, event->state);
}

static void server_cursor_axis(struct wl_listener *listener, void *data) {
	struct gemwl_server *server =
		wl_container_of(listener, server, cursor_axis);
	struct wlr_pointer_axis_event *event = data;
	wlr_seat_pointer_notify_axis(server->seat, event->time_msec,
		event->orientation, event->delta, event->delta_discrete,
		event->source, event->relative_direction);
}

static void server_cursor_frame(struct wl_listener *listener, void *data) {
	struct gemwl_server *server =
		wl_container_of(listener, server, cursor_frame);
	wlr_seat_pointer_notify_frame(server->seat);
}

/* The output's single buffer IS the live framebuffer, read continuously
 * by the panel. The wlroots gles2 renderer only glFlush()s at the end of
 * a frame, so the next frame's clear+draw can start while the previous
 * frame's GPU work is still in flight: stale cursor pixels get drawn
 * after the new clear (cursor trails) and cleared-but-not-yet-drawn
 * regions flash black (triangles while windows move/resize). Force GPU
 * completion before the next frame may touch the fb. (A real fix would
 * be double buffering; this keeps the GPU-direct single-buffer path.) */
static void gemfb_sync_gpu(struct wlr_renderer *renderer) {
	if (!wlr_renderer_is_gles2(renderer)) {
		return;
	}
	struct wlr_egl *egl = wlr_gles2_renderer_get_egl(renderer);
	EGLDisplay display = wlr_egl_get_display(egl);
	EGLContext context = wlr_egl_get_context(egl);
	EGLContext prev_context = eglGetCurrentContext(); /* usually none on this thread */
	if (eglMakeCurrent(display, EGL_NO_SURFACE, EGL_NO_SURFACE, context) != EGL_TRUE) {
		wlr_log(WLR_ERROR, "gemfb: eglMakeCurrent for frame sync failed: 0x%x",
			eglGetError());
		return;
	}
	glFinish();
	/* Restore the previous (usually: no) current context. The renderer
	 * re-binds its own context on the next wlr_renderer_begin. */
	if (eglMakeCurrent(display, EGL_NO_SURFACE, EGL_NO_SURFACE, prev_context) != EGL_TRUE) {
		wlr_log(WLR_ERROR, "gemfb: eglMakeCurrent restore failed: 0x%x",
			eglGetError());
	}
}

static void output_frame(struct wl_listener *listener, void *data) {
	struct gemwl_output *output = wl_container_of(listener, output, frame);
	struct wlr_scene *scene = output->server->scene;

	struct wlr_scene_output *scene_output = wlr_scene_get_scene_output(
		scene, output->wlr_output);
	bool committed = wlr_scene_output_commit(scene_output, NULL);
	if (committed) {
		gemfb_sync_gpu(output->server->renderer);
		if (output->server->double_buffer) {
			gemfb_blit_frame(output->server->renderer, &output->server->blit);
		}
	}

	struct timespec now;
	clock_gettime(CLOCK_MONOTONIC, &now);
	wlr_scene_output_send_frame_done(scene_output, &now);
}

static void output_request_state(struct wl_listener *listener, void *data) {
	struct gemwl_output *output =
		wl_container_of(listener, output, request_state);
	const struct wlr_output_event_request_state *event = data;
	wlr_output_commit_state(output->wlr_output, event->state);
}

static void output_destroy(struct wl_listener *listener, void *data) {
	struct gemwl_output *output = wl_container_of(listener, output, destroy);
	wl_list_remove(&output->frame.link);
	wl_list_remove(&output->request_state.link);
	wl_list_remove(&output->destroy.link);
	wl_list_remove(&output->link);
	free(output);
}

static void server_new_output(struct wl_listener *listener, void *data) {
	struct gemwl_server *server =
		wl_container_of(listener, server, new_output);
	struct wlr_output *wlr_output = data;
	int t_w, t_h;
	wlr_output_transformed_resolution(wlr_output, &t_w, &t_h);
	wlr_log(WLR_INFO, "new output: %s raw %dx%d logical %dx%d "
		"enabled=%d transform=%d render_format=0x%x", wlr_output->name,
		wlr_output->width, wlr_output->height, t_w, t_h,
		wlr_output->enabled, wlr_output->transform, wlr_output->render_format);

	wlr_output_init_render(wlr_output, server->allocator, server->renderer);

	struct wlr_output_state state;
	wlr_output_state_init(&state);
	wlr_output_state_set_enabled(&state, true);
	wlr_output_commit_state(wlr_output, &state);
	wlr_output_state_finish(&state);

	struct gemwl_output *output = calloc(1, sizeof(*output));
	output->wlr_output = wlr_output;
	output->server = server;

	output->frame.notify = output_frame;
	wl_signal_add(&wlr_output->events.frame, &output->frame);
	output->request_state.notify = output_request_state;
	wl_signal_add(&wlr_output->events.request_state, &output->request_state);
	output->destroy.notify = output_destroy;
	wl_signal_add(&wlr_output->events.destroy, &output->destroy);

	wl_list_insert(&server->outputs, &output->link);

	struct wlr_output_layout_output *l_output =
		wlr_output_layout_add_auto(server->output_layout, wlr_output);
	struct wlr_scene_output *scene_output =
		wlr_scene_output_create(server->scene, wlr_output);
	wlr_scene_output_layout_add_output(server->scene_layout,
		l_output, scene_output);
	wlr_log(WLR_INFO, "scene output created for %s", wlr_output->name);
}

/* gemwl is a KIOSK compositor: every mapped toplevel is sized to the full
 * (logical, post-transform) output and presented fullscreen.  KWin nested
 * forces its own size anyway (--width/--height); wlroots wayland-backend
 * compositors (labwc) adopt the size gemwl sends here — without it they
 * stay at their 1280x720 default (the whole point of sizing, 2026-09-02). */
static void gemwl_toplevel_size_to_output(struct gemwl_toplevel *toplevel) {
	struct gemwl_output *output = NULL;
	wl_list_for_each(output, &toplevel->server->outputs, link) {
		break;
	}
	if (output == NULL) {
		return;
	}
	int t_w, t_h;
	wlr_output_transformed_resolution(output->wlr_output, &t_w, &t_h);
	wlr_xdg_toplevel_set_size(toplevel->xdg_toplevel, t_w, t_h);
	wlr_xdg_surface_schedule_configure(toplevel->xdg_toplevel->base);
	wlr_log(WLR_INFO, "gemwl toplevel sized to output %dx%d", t_w, t_h);
}

static void xdg_toplevel_map(struct wl_listener *listener, void *data) {
	struct gemwl_toplevel *toplevel =
		wl_container_of(listener, toplevel, map);
	wl_list_insert(&toplevel->server->toplevels, &toplevel->link);
	gemwl_toplevel_size_to_output(toplevel);
	wlr_log(WLR_INFO, "gemwl toplevel mapped at scene pos (%d, %d)",
		toplevel->scene_tree->node.x, toplevel->scene_tree->node.y);
	focus_toplevel(toplevel, toplevel->xdg_toplevel->base->surface);
}

static void xdg_toplevel_unmap(struct wl_listener *listener, void *data) {
	struct gemwl_toplevel *toplevel =
		wl_container_of(listener, toplevel, unmap);
	wl_list_remove(&toplevel->link);
}

static void xdg_toplevel_commit(struct wl_listener *listener, void *data) {
	struct gemwl_toplevel *toplevel =
		wl_container_of(listener, toplevel, commit);
	if (toplevel->xdg_toplevel->base->initial_commit) {
		gemwl_toplevel_size_to_output(toplevel);
		wlr_log(WLR_INFO, "gemwl toplevel initial commit: current %dx%d "
			"at (%d, %d)", toplevel->xdg_toplevel->current.width,
			toplevel->xdg_toplevel->current.height, toplevel->scene_tree->node.x,
			toplevel->scene_tree->node.y);
	}
}

static void xdg_toplevel_destroy(struct wl_listener *listener, void *data) {
	struct gemwl_toplevel *toplevel =
		wl_container_of(listener, toplevel, destroy);
	wl_list_remove(&toplevel->map.link);
	wl_list_remove(&toplevel->unmap.link);
	wl_list_remove(&toplevel->commit.link);
	wl_list_remove(&toplevel->destroy.link);
	wl_list_remove(&toplevel->request_fullscreen.link);
	free(toplevel);
}

static void xdg_toplevel_request_fullscreen(struct wl_listener *listener, void *data) {
	struct gemwl_toplevel *toplevel =
		wl_container_of(listener, toplevel, request_fullscreen);
	if (toplevel->xdg_toplevel->base->initialized) {
		wlr_xdg_surface_schedule_configure(toplevel->xdg_toplevel->base);
	}
}

static void server_new_xdg_toplevel(struct wl_listener *listener, void *data) {
	struct gemwl_server *server =
		wl_container_of(listener, server, new_xdg_toplevel);
	struct wlr_xdg_toplevel *xdg_toplevel = data;

	struct gemwl_toplevel *toplevel = calloc(1, sizeof(*toplevel));
	toplevel->server = server;
	toplevel->xdg_toplevel = xdg_toplevel;
	toplevel->scene_tree = wlr_scene_xdg_surface_create(
		&toplevel->server->scene->tree, xdg_toplevel->base);
	toplevel->scene_tree->node.data = toplevel;
	xdg_toplevel->base->data = toplevel->scene_tree;

	toplevel->map.notify = xdg_toplevel_map;
	wl_signal_add(&xdg_toplevel->base->surface->events.map, &toplevel->map);
	toplevel->unmap.notify = xdg_toplevel_unmap;
	wl_signal_add(&xdg_toplevel->base->surface->events.unmap, &toplevel->unmap);
	toplevel->commit.notify = xdg_toplevel_commit;
	wl_signal_add(&xdg_toplevel->base->surface->events.commit, &toplevel->commit);

	toplevel->destroy.notify = xdg_toplevel_destroy;
	wl_signal_add(&xdg_toplevel->events.destroy, &toplevel->destroy);

	toplevel->request_fullscreen.notify = xdg_toplevel_request_fullscreen;
	wl_signal_add(&xdg_toplevel->events.request_fullscreen,
		&toplevel->request_fullscreen);
}

static void xdg_popup_commit(struct wl_listener *listener, void *data) {
	struct gemwl_popup *popup = wl_container_of(listener, popup, commit);
	if (popup->xdg_popup->base->initial_commit) {
		wlr_xdg_surface_schedule_configure(popup->xdg_popup->base);
	}
}

static void xdg_popup_destroy(struct wl_listener *listener, void *data) {
	struct gemwl_popup *popup = wl_container_of(listener, popup, destroy);
	wl_list_remove(&popup->commit.link);
	wl_list_remove(&popup->destroy.link);
	free(popup);
}

static void server_new_xdg_popup(struct wl_listener *listener, void *data) {
	struct gemwl_server *server =
		wl_container_of(listener, server, new_xdg_popup);
	struct wlr_xdg_popup *xdg_popup = data;

	struct gemwl_popup *popup = calloc(1, sizeof(*popup));
	popup->xdg_popup = xdg_popup;

	struct wlr_xdg_surface *parent =
		wlr_xdg_surface_try_from_wlr_surface(xdg_popup->parent);
	assert(parent != NULL);
	struct wlr_scene_tree *parent_tree = parent->data;
	xdg_popup->base->data =
		wlr_scene_xdg_surface_create(parent_tree, xdg_popup->base);

	popup->commit.notify = xdg_popup_commit;
	wl_signal_add(&xdg_popup->base->surface->events.commit, &popup->commit);
	popup->destroy.notify = xdg_popup_destroy;
	wl_signal_add(&xdg_popup->events.destroy, &popup->destroy);
}

static int usage(const char *argv0) {
	fprintf(stderr,
		"Usage: %s [-t 0|90|180|270] [-s cmd] [-d]\n"
		"  -t N   output transform (default 90 = landscape desktop)\n"
		"  -s CMD run a Wayland client after startup\n"
		"  -d     don't unbind fbcon\n",
		argv0);
	return 0;
}

int main(int argc, char *argv[]) {
	wlr_log_init(WLR_DEBUG, NULL);
	char *startup_cmd = NULL;
	uint32_t transform = WL_OUTPUT_TRANSFORM_90;
	bool unbind_fbcon = true;

	int c;
	while ((c = getopt(argc, argv, "t:s:dh")) != -1) {
		switch (c) {
		case 't': {
			/* CLI takes degrees; wl_output_transform is an enum:
			 * NORMAL=0, 90=1, 180=2, 270=3. */
			int deg = atoi(optarg);
			if (deg == 0) {
				transform = WL_OUTPUT_TRANSFORM_NORMAL;
			} else if (deg == 90) {
				transform = WL_OUTPUT_TRANSFORM_90;
			} else if (deg == 180) {
				transform = WL_OUTPUT_TRANSFORM_180;
			} else if (deg == 270) {
				transform = WL_OUTPUT_TRANSFORM_270;
			} else {
				transform = WL_OUTPUT_TRANSFORM_90;
			}
			break;
		}
		case 's':
			startup_cmd = optarg;
			break;
		case 'd':
			unbind_fbcon = false;
			break;
		default:
			return usage(argv[0]);
		}
	}

	/* --- gemfb dma-buf: the display this compositor drives --- */
	int gfd = open("/dev/gemfb", O_RDWR);	if (gfd < 0) {
		wlr_log(WLR_ERROR, "Failed to open /dev/gemfb: %s", strerror(errno));
		return 1;
	}
	int dma_fd = ioctl(gfd, GEMFB_IOC_EXPORT);
	close(gfd);
	if (dma_fd < 0) {
		wlr_log(WLR_ERROR, "GEMFB_IOC_EXPORT failed: %s", strerror(errno));
		return 1;
	}
	wlr_log(WLR_INFO, "LK framebuffer dma-buf fd=%d (%dx%d, pitch %d, "
		"transform %u)", dma_fd, FB_W, FB_H, FB_PITCH, transform);

	int drm_fd = open("/dev/dri/renderD129", O_RDWR | O_CLOEXEC);
	if (drm_fd < 0) {
		drm_fd = open("/dev/dri/renderD128", O_RDWR | O_CLOEXEC);
	}
	if (drm_fd < 0) {
		wlr_log(WLR_ERROR, "Failed to open panfrost render node: %s",
			strerror(errno));
		return 1;
	}

	/* --- double-buffered output: allocate the shadow buffer ---
	 * The scene renders into this private dma-buf (not the live fb); a
	 * per-frame GPU blit copies it into the LK fb (gemfb_blit_*).
	 * GEMWL_SINGLE_BUFFER=1 forces the old direct-to-fb path for A/B. */
	bool single_buffer = getenv("GEMWL_SINGLE_BUFFER") != NULL;
	int shadow_fd = -1;
	if (!single_buffer) {
		shadow_fd = gemfb_shadow_alloc(drm_fd);
		if (shadow_fd < 0) {
			wlr_log(WLR_ERROR, "shadow buffer alloc failed — falling back "
				"to single-buffer (direct-to-fb)");
			single_buffer = true;
		}
	}

	struct gemwl_server server = {0};
	server.wl_display = wl_display_create();
	struct wl_event_loop *loop = wl_display_get_event_loop(server.wl_display);

	/* Multi-backend: our gemfb output + libinput input */
	server.backend = wlr_multi_backend_create(loop);
	if (server.backend == NULL) {
		wlr_log(WLR_ERROR, "failed to create multi-backend");
		return 1;
	}

	struct wlr_session *session = wlr_session_create(loop);
	if (session == NULL) {
		wlr_log(WLR_ERROR, "failed to create session (libseat); "
			"input will be unavailable");
	}
	struct wlr_backend *gemfb = gemfb_backend_create(loop, drm_fd, dma_fd,
		transform, unbind_fbcon);
	struct wlr_backend *libinput = NULL;
	if (session != NULL) {
		libinput = wlr_libinput_backend_create(session);
	}
	if (gemfb == NULL) {
		wlr_log(WLR_ERROR, "failed to create gemfb backend");
		return 1;
	}
	wlr_multi_backend_add(server.backend, gemfb);
	if (libinput != NULL) {
		wlr_multi_backend_add(server.backend, libinput);
	} else {
		wlr_log(WLR_ERROR, "no input backend; running display-only");
	}

	server.renderer = wlr_renderer_autocreate(server.backend);
	if (server.renderer == NULL) {
		wlr_log(WLR_ERROR, "failed to create wlr_renderer");
		return 1;
	}
	wlr_renderer_init_wl_display(server.renderer, server.wl_display);

	/* Eager blit init: create the fb/shadow EGL images, fb FBO, shadow
	 * texture and blit program NOW so a failure can fall back to
	 * single-buffer mode cleanly (the allocator is not created yet).
	 * Must run with the renderer's EGL context current. */
	if (!single_buffer && !wlr_renderer_is_gles2(server.renderer)) {
		wlr_log(WLR_ERROR, "renderer is not gles2 — blit unavailable; "
			"falling back to single-buffer (direct-to-fb)");
		single_buffer = true;
	}
	if (!single_buffer) {
		struct wlr_egl *egl = wlr_gles2_renderer_get_egl(server.renderer);
		EGLDisplay display = wlr_egl_get_display(egl);
		EGLContext context = wlr_egl_get_context(egl);
		EGLContext prev_ctx = eglGetCurrentContext();
		EGLSurface prev_draw = eglGetCurrentSurface(EGL_DRAW);
		EGLSurface prev_read = eglGetCurrentSurface(EGL_READ);
		gemfb_eglCreateImageKHR = (PFNEGLCREATEIMAGEKHRPROC)
			eglGetProcAddress("eglCreateImageKHR");
		gemfb_glEGLImageTargetTexture2DOES = (PFNGLEGLIMAGETARGETTEXTURE2DOESPROC)
			eglGetProcAddress("glEGLImageTargetTexture2DOES");
		server.blit.fb_fd = dma_fd;
		server.blit.shadow_fd = shadow_fd;
		bool ok = gemfb_eglCreateImageKHR != NULL &&
			gemfb_glEGLImageTargetTexture2DOES != NULL;
		if (ok && eglMakeCurrent(display, EGL_NO_SURFACE, EGL_NO_SURFACE,
				context) != EGL_TRUE) {
			wlr_log(WLR_ERROR, "blit init: eglMakeCurrent failed: 0x%x",
				eglGetError());
			ok = false;
		}
		if (ok) {
			ok = gemfb_blit_init(&server.blit, display);
		}
		/* restore the previous (usually: no) current context */
		if (eglMakeCurrent(display, prev_draw, prev_read, prev_ctx) != EGL_TRUE) {
			wlr_log(WLR_ERROR, "blit init: context restore failed: 0x%x",
				eglGetError());
		}
		if (!ok) {
			wlr_log(WLR_ERROR, "blit init failed — falling back to "
				"single-buffer (direct-to-fb)");
			single_buffer = true;
		}
	}
	server.double_buffer = !single_buffer;
	server.fb_dma_fd = dma_fd;
	server.shadow_dma_fd = shadow_fd;
	if (server.double_buffer) {
		wlr_log(WLR_INFO, "output mode: DOUBLE-buffered (scene -> shadow, "
			"GPU blit -> LK fb)");
	} else {
		wlr_log(WLR_INFO, "output mode: SINGLE-buffered (direct-to-fb, legacy)");
	}

	server.allocator = gemfb_allocator_create(
		server.double_buffer ? server.shadow_dma_fd : dma_fd);
	if (server.allocator == NULL) {
		wlr_log(WLR_ERROR, "failed to create gemfb allocator");
		return 1;
	}

	wlr_compositor_create(server.wl_display, 5, server.renderer);
	wlr_subcompositor_create(server.wl_display);
	/* Viewporter (stable protocol): REQUIRED by some Wayland clients —
	 * wine's wayland driver (winewayland) refuses to initialise without
	 * wp_viewporter ("Wayland compositor doesn't support wp_viewporter",
	 * observed 2026-09-09 with wine64+box64 — docs/wine-d3d.md). wlroots
	 * 0.18 handles the wp_viewport resources internally; the scene
	 * renderer already accounts for viewports (buffer source box), so
	 * registering the global is the whole job. */
	wlr_viewporter_create(server.wl_display);
	wlr_data_device_manager_create(server.wl_display);

	server.output_layout = wlr_output_layout_create(server.wl_display);

	wl_list_init(&server.outputs);
	server.new_output.notify = server_new_output;
	wl_signal_add(&server.backend->events.new_output, &server.new_output);

	server.scene = wlr_scene_create();
	server.scene_layout = wlr_scene_attach_output_layout(server.scene,
		server.output_layout);

	wl_list_init(&server.toplevels);
	server.xdg_shell = wlr_xdg_shell_create(server.wl_display, 3);
	server.new_xdg_toplevel.notify = server_new_xdg_toplevel;
	wl_signal_add(&server.xdg_shell->events.new_toplevel,
		&server.new_xdg_toplevel);
	server.new_xdg_popup.notify = server_new_xdg_popup;
	wl_signal_add(&server.xdg_shell->events.new_popup,
		&server.new_xdg_popup);

	server.cursor = wlr_cursor_create();
	wlr_cursor_attach_output_layout(server.cursor, server.output_layout);
	/* The Plasma cursor theme ships in the rootfs (breeze-cursor-theme);
	 * NULL theme falls back to "default" which has no cursors here. */
	server.cursor_mgr = wlr_xcursor_manager_create(
		getenv("XCURSOR_THEME") ? : "breeze_cursors", 24);
	if (server.cursor_mgr == NULL) {
		wlr_log(WLR_ERROR, "failed to create xcursor manager (theme) — "
			"pointer will be invisible");
	}

	server.cursor_motion.notify = server_cursor_motion;
	wl_signal_add(&server.cursor->events.motion, &server.cursor_motion);
	server.cursor_motion_absolute.notify = server_cursor_motion_absolute;
	wl_signal_add(&server.cursor->events.motion_absolute,
		&server.cursor_motion_absolute);
	server.cursor_button.notify = server_cursor_button;
	wl_signal_add(&server.cursor->events.button, &server.cursor_button);
	server.cursor_axis.notify = server_cursor_axis;
	wl_signal_add(&server.cursor->events.axis, &server.cursor_axis);
	server.cursor_frame.notify = server_cursor_frame;
	wl_signal_add(&server.cursor->events.frame, &server.cursor_frame);

	wl_list_init(&server.keyboards);
	server.touch_emulated_id = -1;
	server.touch_pointer_emu = getenv("GEMWL_TOUCH_POINTER_EMU") != NULL;
	if (server.touch_pointer_emu) {
		wlr_log(WLR_INFO, "gemwl input: GEMWL_TOUCH_POINTER_EMU=1 - touch drives the pointer (legacy behaviour)");
	}
	server.new_input.notify = server_new_input;
	wl_signal_add(&server.backend->events.new_input, &server.new_input);
	server.seat = wlr_seat_create(server.wl_display, "seat0");
	server.request_cursor.notify = seat_request_cursor;
	wl_signal_add(&server.seat->events.request_set_cursor,
		&server.request_cursor);
	server.request_set_selection.notify = seat_request_set_selection;
	wl_signal_add(&server.seat->events.request_set_selection,
		&server.request_set_selection);

	const char *socket = wl_display_add_socket_auto(server.wl_display);
	if (!socket) {
		wlr_backend_destroy(server.backend);
		return 1;
	}

	wlr_log(WLR_INFO, "gemwl build: %s", GEMINI_BUILD_ID);

	if (!wlr_backend_start(server.backend)) {
		wlr_log(WLR_ERROR, "backend failed to start");
		wlr_backend_destroy(server.backend);
		wl_display_destroy(server.wl_display);
		return 1;
	}

	setenv("WAYLAND_DISPLAY", socket, true);
	if (startup_cmd) {
		if (fork() == 0) {
			execl("/bin/sh", "/bin/sh", "-c", startup_cmd, (void *)NULL);
		}
	}

	wlr_log(WLR_INFO, "gemwl: Wayland compositor running on "
		"WAYLAND_DISPLAY=%s", socket);
	wl_display_run(server.wl_display);

	wl_display_destroy_clients(server.wl_display);
	wlr_scene_node_destroy(&server.scene->tree.node);
	wlr_xcursor_manager_destroy(server.cursor_mgr);
	wlr_cursor_destroy(server.cursor);
	wlr_allocator_destroy(server.allocator);
	wlr_renderer_destroy(server.renderer);
	wlr_backend_destroy(server.backend);
	wl_display_destroy(server.wl_display);
	close(dma_fd);
	close(drm_fd);
	return 0;
}
