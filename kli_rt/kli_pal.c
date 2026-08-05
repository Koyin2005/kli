#include <stdint.h>
#include <stdio.h>
#ifdef _WIN32
    #include <windows.h>
    void kli_print_string(uint8_t *ptr, size_t len){
        if (len == 0){
            return;
        }
        HANDLE stdout_handle = GetStdHandle(STD_OUTPUT_HANDLE);
        DWORD mode;
        // Is stdout attached to a console
        if (GetConsoleMode(stdout_handle, &mode))
        {
            wchar_t buffer[4096];
            // Convert UTF-8 -> UTF-16
            int wide_len = MultiByteToWideChar(
                CP_UTF8,
                MB_ERR_INVALID_CHARS,
                ptr,
                len,
                buffer,
                sizeof(buffer));

            if (wide_len == 0){
                return;
            }

            DWORD written;
            WriteConsoleW(
                stdout_handle,
                buffer,
                wide_len,
                &written,
                NULL);

        }
        else
        {
            fwrite(ptr, 1,len, stdout);
            fflush(stdout);
        }
    }
#else 
    void kli_print_string(uint8_t *ptr, size_t len){
        fwrite(ptr,1,len,stdout);
        fflush(stdout);
    }
#endif