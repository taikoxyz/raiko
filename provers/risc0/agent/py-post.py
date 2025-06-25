import requests
import json

# 读取二进制文件
with open('tests/fixtures/input-1306738.bin', 'rb') as f:
    binary_data = f.read()

# 转换为bytes数组
input_bytes = list(binary_data)

# 发送请求
response = requests.post(
    "http://localhost:9999/proof",
    json={
        "input": input_bytes,
        "proof_type": "Batch"
    }
)

print(response.json())