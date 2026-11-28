---
translatedFrom: abfd698ccd05
---

# Tên và lỗi kiểm tra hợp lệ

Khi từ chối một tên hay một định danh, Dymini Panel trả về lý do. Panel hiển thị đúng thông báo đó của API thay vì đổi thành một lỗi lưu chung chung; bên gọi qua MCP cũng nhận cùng lý do. Lỗi trùng tên nghĩa là tên đó đã được dùng trong phạm vi tương ứng. Hãy chọn tên khác, đừng đổi cách viết để lách quy tắc.

## Tên tài nguyên

Máy chủ, project, environment, service (kể cả database và compose stack), bản cài template/plugin, registry, nhà cung cấp Git, chứng chỉ, đích S3 và tên compose project dùng **1–63 ký tự ASCII gồm chữ thường, chữ số hoặc dấu gạch ngang**. Ký tự đầu phải là chữ hoặc số. Không nhận khoảng trắng, chữ hoa, dấu gạch dưới và chữ có dấu. Ví dụ: dùng `my-app` chứ không phải `My App` hay `my_app`. Các biểu mẫu tương ứng ghi quy tắc này bằng tiếng Anh hoặc tiếng Việt.

## Định danh khác và nhãn hiển thị

Những trường này cố ý có quy tắc khác; không phải trường nào tên là “Name” cũng theo quy tắc tên tài nguyên:

| Trường                                                                      | Quy tắc                                                                                                                                                                     |
| --------------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Tên miền / host của panel / cổng Access / host Turnstile                    | 1–253 ký tự ASCII, có ít nhất một dấu chấm, mỗi nhãn 1–63 chữ/số/gạch ngang; nhãn không bắt đầu hay kết thúc bằng gạch ngang; không có scheme, cổng, đường dẫn hay wildcard |
| Compose service                                                             | 1–63 ký tự ASCII gồm chữ/số/dấu chấm/gạch dưới/gạch ngang; bắt đầu bằng chữ hoặc số                                                                                         |
| Tên database và username                                                    | 1–63 ký tự ASCII gồm chữ/số/gạch dưới; bắt đầu bằng chữ                                                                                                                     |
| Khóa biến môi trường                                                        | 1–128 ký tự ASCII gồm chữ/số/gạch dưới; bắt đầu bằng chữ hoặc gạch dưới                                                                                                     |
| Slug phòng ban/cấp bậc của Access                                           | Bỏ khoảng trắng hai đầu và chuyển chữ thường, 1–48 chữ/số/gạch ngang; không bắt đầu hay kết thúc bằng gạch ngang                                                            |
| Tên phòng ban/cấp bậc của Access, tên giới hạn tần suất, tên agent đã duyệt | Không rỗng, tối đa 80 byte UTF-8 sau khi bỏ khoảng trắng hai đầu, không có ký tự điều khiển                                                                                 |
| Tên token                                                                   | Không rỗng, tối đa 64 byte UTF-8 sau khi bỏ khoảng trắng hai đầu, không có ký tự điều khiển                                                                                 |
| Tên chính sách cache, cảnh báo, kênh thông báo                              | Không rỗng, tối đa 100 byte UTF-8 sau khi bỏ khoảng trắng hai đầu                                                                                                           |
| Tên tác vụ sao lưu                                                          | Không rỗng, tối đa 100 byte UTF-8 tính cả khoảng trắng hai đầu                                                                                                              |
| S3 bucket                                                                   | 1–63 ký tự ASCII gồm chữ thường/số/gạch ngang/dấu chấm                                                                                                                      |

Giới hạn byte UTF-8 không phải số ký tự: chữ có dấu có thể chiếm nhiều byte. Tên hiển thị của người dùng không phải tên tài nguyên; chúng giữ cách chuẩn hóa cũ (tên hiển thị không rỗng của người dùng panel được bỏ khoảng trắng hai đầu và cắt còn 80 ký tự). Template và plugin báo tên biến cùng với quy tắc tên miền/email/giá trị. Cơ chế chống trùng tên và tên dành riêng hiện có không đổi, kể cả compose project đã thuộc một stack khác và các biến image dành riêng của plugin.

Nhánh Git, tên GitHub App/tổ chức, tên đăng nhập Unix, tên volume, tiền tố đối tượng/mẫu tên file sao lưu, tên query/cookie của cache, username registry, username basic-auth, tag image và địa chỉ email cũng báo giới hạn riêng về độ dài, ký tự hoặc giá trị dành riêng. Việc kiểm tra không tự đổi tên đầu vào và không nới lỏng quy tắc đặt tên.
