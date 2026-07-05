// Case A (reduced): an #if in ctor-initializer (DECLARATION) position.
// tree-sitter recovers the #if-guarded init list as ERROR-wrapped bogus
// field declarations, inventing phantom members (start_position /
// end_position) and corrupting the real ones. Slice 1's declaration-
// position directive repair blanks the #if/#endif lines so the ctor parses:
// no phantom members, and members AFTER the ctor still attribute to Widget.
class Widget {
  public:
    int early_field;

    Widget(const Widget& val)
#if DIAG_POSITIONS
        : start_position(val.start_pos()),
          end_position(val.end_pos())
#endif
    {
        do_setup();
    }

    int late_field;
    void late_method() { return; }
};

void use_widget() {
    Widget w;
    w.late_field = 1;
}
