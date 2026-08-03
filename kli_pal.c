#include <stdint.h>
#include <stdio.h>
#ifdef _WIN32
    #include <windows.h>
    void kli_print_string(uint8_t *ptr, size_t len){
        HANDLE stdout_handle = GetStdHandle(STD_OUTPUT_HANDLE);
        DWORD mode;
        // Is stdout attached to a console
        if (GetConsoleMode(stdout_handle, &mode))
        {
            // Convert UTF-8 -> UTF-16
            int wide_len = MultiByteToWideChar(
                CP_UTF8,
                MB_ERR_INVALID_CHARS,
                ptr,
                -1,
                NULL,
                0);

            if (wide_len == 0)
                return;

            wchar_t *wide = malloc(wide_len * sizeof(wchar_t));
            if (!wide) return;

            if (MultiByteToWideChar(
                    CP_UTF8,
                    MB_ERR_INVALID_CHARS,
                    ptr,
                    -1,
                    wide,
                    wide_len) == 0)
            {
                free(wide);
                return;
            }

            DWORD written;
            WriteConsoleW(
                stdout_handle,
                wide,
                wide_len,
                &written,
                NULL);

            free(wide);
        }
        else
        {
            fwrite(ptr, 1,len, stdout);
        }
        fflush(stdout);
    }
#else 
    void kli_print_string(uint8_t *ptr, size_t len){
        fwrite(ptr,1,len,stdout);
        fflush(stdout);
    }
#endif