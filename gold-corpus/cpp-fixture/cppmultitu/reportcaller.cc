#include "cppmultitu/report.h"
namespace rpt {
void trigger() {
  report_error(1, "boom");
}
}  // namespace rpt
