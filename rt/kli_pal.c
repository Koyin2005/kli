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
    size_t kli_read_string(uint8_t *ptr, size_t len) {
        if (!len){
            return 0;
        }
        HANDLE stdin_handle = GetStdHandle(STD_INPUT_HANDLE);
        DWORD mode;
        // Is stdin attached to a console
        if (GetConsoleMode(stdin_handle, &mode))
        {
            #define BUF_SIZE 4096
            wchar_t buffer[BUF_SIZE];

            CONSOLE_READCONSOLE_CONTROL control = (CONSOLE_READCONSOLE_CONTROL){
                .nLength = sizeof(CONSOLE_READCONSOLE_CONTROL),
                .nInitialChars = 0,
                .dwControlKeyState = 0,
                .dwCtrlWakeupMask = 1 << 0x1A
            };
            DWORD read;
            uint8_t succeeded = ReadConsoleW(
                stdin_handle,
                buffer,
                BUF_SIZE - 1,
                &read,
                &control);
            
            
            if (!succeeded) {
                return 0;
            } 
            while (read > 0 &&
               (buffer[read - 1] == L'\n' ||
                buffer[read - 1] == L'\r'))
            {
                read--;
            }
            int byte_len = WideCharToMultiByte(
                CP_UTF8,
                WC_ERR_INVALID_CHARS,
                buffer,
                read,
                ptr,
                len,
                NULL,
                NULL
            );
            return byte_len;
        }
        else
        {
            return fread(ptr, 1,len, stdin);
        }
    }
#else 
    void kli_print_string(uint8_t *ptr, size_t len){
        fwrite(ptr,1,len,stdout);
        fflush(stdout);
    }

    uint8_t kli_read_byte() {
        return getchar()
    }
#endif