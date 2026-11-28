---
translatedFrom: 20b320af9465
---

# Mở panel qua HTTPS

Bản cài mới trả lời ở `http://<ip-máy-chủ>:3000`. Qua HTTP thường, mật khẩu đăng nhập, cookie phiên và mọi bí mật nhập vào panel (biến môi trường, thông tin đăng nhập registry và S3, token Git) đều đi qua mạng dưới dạng chữ thường, ai chặn được đường truyền đều đọc được.

## Panel xử lý thế nào

- Một banner cảnh báo luôn nằm ở đầu mọi trang, kể cả màn đăng nhập và màn thiết lập, khi `location.protocol` là `http:` và host không phải loopback (`localhost`, `*.localhost`, `127.x.x.x`, `::1`). Ở các trang sau khi đăng nhập, banner dẫn tới **Certificates**.
- Lưu một bí mật (biến môi trường hoặc nhập `.env`, thông tin đăng nhập registry, đích S3 hoặc nhà cung cấp Git, và ô _Check value_) sẽ hỏi xác nhận trước. Câu trả lời chỉ được nhớ trong `sessionStorage` của tab trình duyệt đó, nên đóng tab là sẽ hỏi lại.
- **Certificates → Panel domain** hiện cùng cảnh báo kèm các bước bên dưới. Panel không chặn gì: API vẫn nhận request qua HTTP, vì cài đặt sau một mạng riêng tin cậy hoặc sau proxy kết thúc TLS rồi chuyển tiếp HTTP thường là hợp lệ. Đặt `DYMINI_PANEL_COOKIE_SECURE=true` khi panel chỉ còn truy cập được qua HTTPS.

## Đặt tên miền riêng cho panel

1. Trỏ một bản ghi DNS cho panel (ví dụ `panel.example.com`) về máy chủ.
2. **Certificates → Panel domain**: nhập tên miền, chọn chế độ SSL (Let's Encrypt hoặc chứng chỉ tải lên) rồi lưu.
3. Mở panel qua `https://panel.example.com` và đặt `DYMINI_PANEL_PUBLIC_URL` thành địa chỉ đó. GitHub App cũng nên được tạo từ địa chỉ HTTPS, để URL callback và webhook của nó dùng địa chỉ này.
4. Bấm **Check SSL** dưới biểu mẫu. Dymini Panel kết nối tới tên miền đã lưu qua HTTPS và cho biết chứng chỉ có được tin cậy không, có phủ tên miền không, ai cấp và còn bao nhiêu ngày. Let's Encrypt có thể mất khoảng một phút để cấp: nếu ngay sau khi lưu mà kiểm tra báo chứng chỉ chưa được tin cậy, hãy đợi rồi kiểm tra lại. AI Agent nhận cùng kết quả qua tool `get_v1_panel_ssl-check`.

Tên miền panel đã lưu cũng là địa chỉ để AI Agent đăng nhập (xem [AI Agent](mcp.md)).
