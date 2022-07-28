import 'package:flowy_editor/document/node.dart';
import 'package:flowy_editor/document/position.dart';
import 'package:flowy_editor/document/selection.dart';
import 'package:flowy_editor/document/text_delta.dart';
import 'package:flowy_editor/editor_state.dart';
import 'package:flowy_editor/document/path.dart';
import 'package:flowy_editor/render/node_widget_builder.dart';
import 'package:flowy_editor/render/render_plugins.dart';
import 'package:flowy_editor/render/rich_text/rich_text_style.dart';
import 'package:flowy_editor/render/selection/selectable.dart';

import 'package:flutter/material.dart';
import 'package:flutter/rendering.dart';

class RichTextNodeWidgetBuilder extends NodeWidgetBuilder {
  RichTextNodeWidgetBuilder.create({
    required super.editorState,
    required super.node,
    required super.key,
  }) : super.create();

  @override
  Widget build(BuildContext context) {
    return FlowyRichText(
      key: key,
      textNode: node as TextNode,
      editorState: editorState,
    );
  }
}

typedef FlowyTextSpanDecorator = TextSpan Function(TextSpan textSpan);

class FlowyRichText extends StatefulWidget {
  const FlowyRichText({
    Key? key,
    this.cursorHeight,
    this.cursorWidth = 2.0,
    this.textSpanDecorator,
    required this.textNode,
    required this.editorState,
  }) : super(key: key);

  final double? cursorHeight;
  final double cursorWidth;
  final TextNode textNode;
  final EditorState editorState;
  final FlowyTextSpanDecorator? textSpanDecorator;

  @override
  State<FlowyRichText> createState() => _FlowyRichTextState();
}

class _FlowyRichTextState extends State<FlowyRichText> with Selectable {
  final _textKey = GlobalKey();

  RenderParagraph get _renderParagraph =>
      _textKey.currentContext?.findRenderObject() as RenderParagraph;

  @override
  Widget build(BuildContext context) {
    return _buildRichText(context);
  }

  @override
  Position start() => Position(path: widget.textNode.path, offset: 0);

  @override
  Position end() => Position(
      path: widget.textNode.path, offset: widget.textNode.toRawString().length);

  @override
  Rect getCursorRectInPosition(Position position) {
    final textPosition = TextPosition(offset: position.offset);
    final cursorOffset =
        _renderParagraph.getOffsetForCaret(textPosition, Rect.zero);
    final cursorHeight = widget.cursorHeight ??
        _renderParagraph.getFullHeightForCaret(textPosition) ??
        5.0; // default height
    return Rect.fromLTWH(
      cursorOffset.dx - (widget.cursorWidth / 2),
      cursorOffset.dy,
      widget.cursorWidth,
      cursorHeight,
    );
  }

  @override
  Position getPositionInOffset(Offset start) {
    final offset = _renderParagraph.globalToLocal(start);
    final baseOffset = _renderParagraph.getPositionForOffset(offset).offset;
    return Position(path: widget.textNode.path, offset: baseOffset);
  }

  @override
  List<Rect> getRectsInSelection(Selection selection) {
    assert(pathEquals(selection.start.path, selection.end.path) &&
        pathEquals(selection.start.path, widget.textNode.path));

    final textSelection = TextSelection(
      baseOffset: selection.start.offset,
      extentOffset: selection.end.offset,
    );
    return _renderParagraph
        .getBoxesForSelection(textSelection)
        .map((box) => box.toRect())
        .toList();
  }

  @override
  Selection getSelectionInRange(Offset start, Offset end) {
    final localStart = _renderParagraph.globalToLocal(start);
    final localEnd = _renderParagraph.globalToLocal(end);
    final baseOffset = _renderParagraph.getPositionForOffset(localStart).offset;
    final extentOffset = _renderParagraph.getPositionForOffset(localEnd).offset;
    return Selection.single(
      path: widget.textNode.path,
      startOffset: baseOffset,
      endOffset: extentOffset,
    );
  }

  Widget _buildRichText(BuildContext context) {
    return _buildSingleRichText(context);
  }

  Widget _buildSingleRichText(BuildContext context) {
    final textSpan = _textSpan;
    return RichText(
      key: _textKey,
      text: widget.textSpanDecorator != null
          ? widget.textSpanDecorator!(textSpan)
          : textSpan,
    );
  }

  // unused now.
  Widget _buildRichTextWithChildren(BuildContext context) {
    return Column(
      crossAxisAlignment: CrossAxisAlignment.start,
      children: [
        _buildSingleRichText(context),
        ...widget.textNode.children
            .map(
              (child) => widget.editorState.renderPlugins.buildWidget(
                context: NodeWidgetContext(
                  buildContext: context,
                  node: child,
                  editorState: widget.editorState,
                ),
              ),
            )
            .toList()
      ],
    );
  }

  TextSpan get _textSpan => TextSpan(
      children: widget.textNode.delta.operations
          .whereType<TextInsert>()
          .map((insert) => RichTextStyle(
                attributes: insert.attributes ?? {},
                text: insert.content,
              ).toTextSpan())
          .toList(growable: false));
}
