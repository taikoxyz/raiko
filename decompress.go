package main

import (
	"bytes"
	"compress/zlib"
	"encoding/hex"
	"fmt"
	"io"
	"os"
)

func readHexFile(filename string) ([]byte, error) {
    hexData, err := os.ReadFile(filename)
    if err != nil {
        return nil, fmt.Errorf("文件读取错误: %w", err)
    }

    decoded := make([]byte, hex.DecodedLen(len(hexData)))
    n, err := hex.Decode(decoded, hexData)
    if err != nil {
        return nil, fmt.Errorf("HEX解码失败: %w", err)
    }
    return decoded[:n], nil
}

func main() {
    compressed, err := readHexFile("core/compressed_blob.hex")
    if err != nil {
        fmt.Println("读取文件失败:", err)
        return
    }
    fmt.Printf("compressed: %d bytes\n", len(compressed))
    fmt.Printf("compressed: %v\n", compressed)

    decompressed, err := decompressData(compressed)
    if err != nil {
        fmt.Println("解压失败:", err)
        return
    }

    fmt.Printf("解压数据: %d bytes\n", len(decompressed))
}

func decompressData(compressed []byte) ([]byte, error) {
    reader := bytes.NewReader(compressed)
    
    zlibReader, err := zlib.NewReader(reader)
    if err != nil {
        return nil, fmt.Errorf("创建解压器失败: %w", err)
    }
    defer zlibReader.Close()

    result, err := io.ReadAll(zlibReader)
    if err != nil {
        return nil, fmt.Errorf("读取解压数据失败: %w", err)
    }

    return result, nil
}
