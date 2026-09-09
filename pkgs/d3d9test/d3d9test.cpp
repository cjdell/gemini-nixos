/* d3d9test.cpp — a minimal Windows Direct3D 9 smoke test.
 *
 * Purpose: prove the wine64 + box64 (x86-64 -> aarch64) stack on the
 * Gemini PDA renders real D3D9. Draws a rotating, vertex-coloured cube
 * with a GDI FPS overlay; logs the D3D9 adapter/device identity to
 * stderr at startup (the "receipt" for the test).
 *
 * Deliberately dependency-light: d3d9 + user32 + gdi32 only (no D3DX,
 * no textures, no audio). The vertex format is the FIXED
 * D3DFVF_XYZ | D3DFVF_DIFFUSE (mingw-w64 d3d9types.h — the flexible
 * FVF macros are not in wine's headers), so no FVF-table risk under
 * wined3d.
 *
 * Build: wineg++ -m64 (from the nixpkgs x86_64 wine64 package) — see
 * pkgs/d3d9test.nix. C++ because wine's headers expose the D3D9 COM
 * interfaces as C++ classes only in C++ mode (in C they are opaque
 * structs needing IDirect3D9_Release()-style macros).
 * Run:  box64 <wine64>/bin/.wine d3d9test.exe (see bin/wine-x86-deploy.sh).
 *
 * Exit: ESC or window close.
 */
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <cstdio>
#include <cstdlib> /* getenv */
#include <cstring>
#include <cmath>
#include <d3d9.h>

static LPDIRECT3D9 g_d3d;
static LPDIRECT3DDEVICE9 g_dev;
static HWND g_hwnd;
static HINSTANCE g_hInst;
static LPDIRECT3DVERTEXBUFFER9 g_vb;
static LPDIRECT3DINDEXBUFFER9 g_ib;
static HFONT g_font;

#define WIN_W 800
#define WIN_H 600

/* Fixed FVF: D3DVECTOR position + D3DCOLOR diffuse (20-byte stride).
 * D3DFVF_XYZ = 0x0002, D3DFVF_DIFFUSE = 0x0040 (mingw-w64 14.0.0). */
typedef struct {
    D3DVECTOR p;
    DWORD     c;
} VTX;
#define TEST_FVF (D3DFVF_XYZ | D3DFVF_DIFFUSE)

static void logmsg(const char *fmt, ...)
{
    va_list ap;
    va_start(ap, fmt);
    fprintf(stderr, "d3d9test: ");
    vfprintf(stderr, fmt, ap);
    fprintf(stderr, "\n");
    fflush(stderr);
    va_end(ap);
}

static DWORD rgb(int r, int g, int b)
{
    return 0xff000000 | (b << 16) | (g << 8) | r;
}

/* 6 faces x 4 vertices, one colour per face. Winding does not matter:
 * culling is disabled. */
static const VTX CUBE[24] = {
    /* -Z (back) */
    {{-1,-1,-1}, rgb(200, 40, 200)}, {{ 1,-1,-1}, rgb(200, 40, 200)},
    {{ 1, 1,-1}, rgb(200, 40, 200)}, {{-1, 1,-1}, rgb(200, 40, 200)},
    /* +Z (front) */
    {{-1,-1, 1}, rgb( 60,200,  60)}, {{ 1,-1, 1}, rgb( 60,200,  60)},
    {{ 1, 1, 1}, rgb( 60,200,  60)}, {{-1, 1, 1}, rgb( 60,200,  60)},
    /* +X (right) */
    {{ 1,-1, 1}, rgb(220,220,  40)}, {{ 1,-1,-1}, rgb(220,220,  40)},
    {{ 1, 1,-1}, rgb(220,220,  40)}, {{ 1, 1, 1}, rgb(220,220,  40)},
    /* -X (left) */
    {{-1,-1,-1}, rgb( 40,160, 220)}, {{-1,-1, 1}, rgb( 40,160, 220)},
    {{-1, 1, 1}, rgb( 40,160, 220)}, {{-1, 1,-1}, rgb( 40,160, 220)},
    /* +Y (top) */
    {{-1, 1, 1}, rgb(240,240, 240)}, {{ 1, 1, 1}, rgb(240,240, 240)},
    {{ 1, 1,-1}, rgb(240,240, 240)}, {{-1, 1,-1}, rgb(240,240, 240)},
    /* -Y (bottom) */
    {{-1,-1,-1}, rgb(220,120,  40)}, {{ 1,-1,-1}, rgb(220,120,  40)},
    {{ 1,-1, 1}, rgb(220,120,  40)}, {{-1,-1, 1}, rgb(220,120,  40)},
};
static const WORD CUBE_IDX[36] = {
    0,1,2, 0,2,3, 4,5,6, 4,6,7, 8,9,10, 8,10,11,
    12,13,14, 12,14,15, 16,17,18, 16,18,19, 20,21,22, 20,22,23,
};

/* D3DMATRIX in wine's headers is an anonymous union { m[4][4]; _11.._44 }
 * (DUMMYSTRUCTNAME/DUMMYUNIONNAME are empty macros) — row-major m[row][col]. */

/* Left-handed perspective projection, D3D depth range [0,1]. */
static void perspective(D3DMATRIX *m, float fovY, float aspect, float zn, float zf)
{
    float y = 1.0f / tanf(fovY * 0.5f);
    float z = zf / (zn - zf);
    ZeroMemory(m, sizeof(*m));
    m->m[0][0] = y / aspect;
    m->m[1][1] = y;
    m->m[2][2] = z;
    m->m[2][3] = -1.0f;
    m->m[3][2] = zn * z;
}

static void rotationXY(D3DMATRIX *m, float ax, float ay)
{
    float cx = cosf(ax), sx = sinf(ax), cy = cosf(ay), sy = sinf(ay);
    /* R = Rx(ax) * Ry(ay) */
    m->m[0][0] = cy;      m->m[0][1] = 0;      m->m[0][2] = sy;      m->m[0][3] = 0;
    m->m[1][0] = sx*sy;   m->m[1][1] = cx;     m->m[1][2] = -sx*cy;  m->m[1][3] = 0;
    m->m[2][0] = -cx*sy;  m->m[2][1] = sx;     m->m[2][2] = cx*cy;   m->m[2][3] = 0;
    m->m[3][0] = 0;       m->m[3][1] = 0;      m->m[3][2] = 0;       m->m[3][3] = 1;
}

int WINAPI WinMain(HINSTANCE hInstance, HINSTANCE pPrev, LPSTR lpCmd, int nShow)
{
    g_hInst = hInstance;

    /* ---- window ---- */
    WNDCLASSEXA wc = { sizeof(wc) };
    wc.style = CS_HREDRAW | CS_VREDRAW;
    wc.lpfnWndProc = DefWindowProcA;
    wc.hInstance = hInstance;
    wc.hCursor = LoadCursor(NULL, IDC_ARROW);
    wc.hbrBackground = (HBRUSH)GetStockObject(BLACK_BRUSH);
    wc.lpszClassName = "D3D9TestClass";
    RegisterClassExA(&wc);
    g_hwnd = CreateWindowExA(0, "D3D9TestClass", "D3D9 Test (wine64 + box64)",
                             WS_OVERLAPPEDWINDOW, CW_USEDEFAULT, CW_USEDEFAULT,
                             WIN_W, WIN_H, NULL, NULL, hInstance, NULL);
    if (!g_hwnd) { logmsg("CreateWindow failed: %lu", GetLastError()); return 1; }
    ShowWindow(g_hwnd, SW_SHOW);
    UpdateWindow(g_hwnd);

    /* ---- D3D9 identity (the receipt) ---- */
    g_d3d = Direct3DCreate9(D3D_SDK_VERSION);
    if (!g_d3d) { logmsg("Direct3DCreate9 failed"); return 1; }

    /* ---- device (HAL first, REF as the last resort) ----
     * NOTE: the adapter/display-mode/caps queries are deferred to AFTER
     * the first frame (log_adapter_info) — on wine 11's wayland driver,
     * GetAdapterDisplayMode page-faulted at startup under box64
     * (2026-09-09, no xdg_output on gemwl) — we want the first frame
     * as the receipt even if that query dies. */
    D3DPRESENT_PARAMETERS pp;
    ZeroMemory(&pp, sizeof(pp));
    pp.hDeviceWindow = g_hwnd;
    pp.Windowed = TRUE;
    pp.SwapEffect = D3DSWAPEFFECT_FLIP;
    pp.BackBufferCount = 2;
    pp.BackBufferFormat = D3DFMT_X8R8G8B8;
    pp.EnableAutoDepthStencil = TRUE;
    pp.AutoDepthStencilFormat = D3DFMT_D24S8;
    pp.MultiSampleType = D3DMULTISAMPLE_NONE;
    pp.PresentationInterval = D3DPRESENT_INTERVAL_DEFAULT;

    HRESULT hr = g_d3d->CreateDevice(0, D3DDEVTYPE_HAL, g_hwnd,
                                     D3DCREATE_HARDWARE_VERTEXPROCESSING, &pp, &g_dev);
    if (FAILED(hr))
        logmsg("CreateDevice(HAL,HWVP) failed 0x%08lx — trying REF", (unsigned long)hr);
    if (FAILED(hr))
        hr = g_d3d->CreateDevice(0, D3DDEVTYPE_REF, g_hwnd, 0, &pp, &g_dev);
    if (FAILED(hr)) { logmsg("CreateDevice failed: 0x%08lx", (unsigned long)hr); return 1; }
    logmsg("device created OK (%s)", hr == D3D_OK ? "HAL/panfrost-or-llvmpipe" : "fallback");
    g_dev->SetRenderState(D3DRS_CULLMODE, D3DCULL_NONE);
    g_dev->SetRenderState(D3DRS_ZENABLE, TRUE);
    g_dev->SetRenderState(D3DRS_LIGHTING, FALSE);

    /* ---- cube resources ---- */
    if (FAILED(g_dev->CreateVertexBuffer(sizeof(CUBE), D3DUSAGE_WRITEONLY,
                                         TEST_FVF, D3DPOOL_MANAGED, &g_vb, NULL)) ||
        FAILED(g_dev->CreateIndexBuffer(sizeof(CUBE_IDX), D3DUSAGE_WRITEONLY,
                                        D3DFMT_INDEX16, D3DPOOL_MANAGED, &g_ib, NULL))) {
        logmsg("resource creation failed");
        return 1;
    }
    void *vb; g_vb->Lock(0, 0, &vb, 0);
    memcpy(vb, CUBE, sizeof(CUBE));
    g_vb->Unlock();
    void *ib; g_ib->Lock(0, 0, &ib, 0);
    memcpy(ib, CUBE_IDX, sizeof(CUBE_IDX));
    g_ib->Unlock();

    /* ---- projection + view (camera at +Z looking at origin) ---- */
    D3DMATRIX proj, view, world;
    perspective(&proj, 70.0f * 3.14159265f / 180.0f, (float)WIN_W / WIN_H, 0.1f, 100.0f);
    ZeroMemory(&view, sizeof(view));
    view.m[0][0] = 1; view.m[1][1] = 1; view.m[2][2] = 1; view.m[3][3] = 1;
    view.m[3][2] = -5.0f; /* camera at z = -5 looking at origin */
    g_dev->SetTransform(D3DTS_PROJECTION, &proj);
    g_dev->SetTransform(D3DTS_VIEW, &view);

    D3DVIEWPORT9 vp = { 0, 0, WIN_W, WIN_H, 0.0f, 1.0f };
    g_dev->SetViewport(&vp);

    /* ---- FPS overlay font ---- */
    /* 0x01 = DEFAULT_SWISS (the macro is not in wine's headers). */
    g_font = CreateFontA(-18, 0, 0, 0, FW_BOLD, FALSE, FALSE, FALSE,
                         DEFAULT_CHARSET, OUT_DEFAULT_PRECIS, CLIP_DEFAULT_PRECIS,
                         CLEARTYPE_QUALITY, 0x01, "Consolas");

    logmsg("render loop start (ESC to quit)");
    LARGE_INTEGER fqc, t0, t1, last;
    QueryPerformanceFrequency(&fqc);
    QueryPerformanceCounter(&t0);
    last = t0;
    long frames = 0, fps = 0;
    int first = 1;

    for (;;) {
        /* input (poll, no blocking) */
        MSG msg;
        while (PeekMessageA(&msg, NULL, 0, 0, PM_REMOVE)) {
            if (msg.message == WM_QUIT) goto done;
            if (msg.message == WM_KEYDOWN && (msg.wParam == VK_ESCAPE)) goto done;
            TranslateMessage(&msg);
            DispatchMessageA(&msg);
        }

        /* draw */
        QueryPerformanceCounter(&t1);
        double secs = (double)(t1.QuadPart - t0.QuadPart) / fqc.QuadPart;
        rotationXY(&world, (float)(secs * 0.9), (float)(secs * 1.3));
        g_dev->SetTransform(D3DTS_WORLD, &world);

        if (FAILED(g_dev->Clear(0, NULL, D3DCLEAR_TARGET | D3DCLEAR_ZBUFFER, 0xFF101418, 1.0f, 0)))
            logmsg("Clear failed");
        if (SUCCEEDED(g_dev->BeginScene())) {
            g_dev->SetStreamSource(0, g_vb, 0, sizeof(VTX));
            g_dev->SetFVF(TEST_FVF);
            g_dev->SetIndices(g_ib);
            g_dev->DrawIndexedPrimitive(D3DPT_TRIANGLELIST, 0, 0, 6, 0, 36);
            g_dev->EndScene();
            g_dev->Present(NULL, NULL, NULL, NULL);
            frames++;
            if (first) {
                first = 0;
                logmsg("FIRST FRAME PRESENTED — D3D9 rendering under wine64+box64");
            }
            /* Adapter identity queries — OFF by default: on wine 11's
             * wayland driver, GetAdapterDisplayMode/GetDeviceCaps page-
             * fault (garbage pointer read) under box64 (2026-09-09;
             * gemwl has no xdg_output — wine's screen struct is partly
             * uninitialised). Opt in with D3D9TEST_IDENTITY=1; the
             * GL renderer receipt comes from WINEDEBUG=+d3d9 instead. */
            if (frames == 2 && getenv("D3D9TEST_IDENTITY")) {
                D3DADAPTER_IDENTIFIER9 id;
                ZeroMemory(&id, sizeof(id));
                if (SUCCEEDED(g_d3d->GetAdapterIdentifier(0, 0, &id)))
                    logmsg("adapter: %s | %s | driver %s", id.Description, id.DeviceName, id.DriverVersion);
            }

            /* GDI FPS overlay after the swap */
            if (t1.QuadPart - last.QuadPart >= fqc.QuadPart) {
                fps = frames; frames = 0; last = t1;
                HDC hdc = GetDC(g_hwnd);
                if (hdc) {
                    SetBkMode(hdc, TRANSPARENT);
                    SetTextColor(hdc, RGB(255, 240, 80));
                    SelectObject(hdc, g_font);
                    RECT rc = { 12, 10, WIN_W - 12, 44 };
                    char buf[96];
                    wsprintfA(buf, "D3D9 OK  |  %ld FPS  |  wine64+box64", fps);
                    DrawTextA(hdc, buf, -1, &rc, DT_LEFT | DT_TOP | DT_SINGLELINE);
                    ReleaseDC(g_hwnd, hdc);
                    logmsg("frame: %ld fps", fps);
                }
            }
        } else {
            logmsg("BeginScene failed");
        }
    }
done:
    logmsg("exit — total frames: %ld", frames);
    if (g_dev) g_dev->Release();
    if (g_vb) g_vb->Release();
    if (g_ib) g_ib->Release();
    if (g_d3d) g_d3d->Release();
    return 0;
}
